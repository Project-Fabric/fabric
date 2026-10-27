require "test_helper"
require_relative "../support/synthetic_identity"

class IdentityAccessTest < ActionDispatch::IntegrationTest
  include SyntheticIdentity
  setup { host! "127.0.0.1" }

  test "unconfigured or malformed tokens fail without leaking configuration or credentials" do
    verifier = IdentityTokenVerifier
    original = verifier::CONFIG_NAMES.to_h { |name| [ name, ENV[name] ] }
    verifier::CONFIG_NAMES.each { |name| ENV.delete(name) }
    get "/api/v1/me"
    assert_response :service_unavailable
    assert_equal "identity_unconfigured", response.parsed_body["error"]
    with_identity_provider do
      get "/api/v1/me", headers: { "Authorization" => "Bearer synthetic-private-token" }
      assert_response :unauthorized
      assert_equal "invalid_token", response.parsed_body["error"]
      refute_includes response.body, "synthetic-private-token"
      assert_equal "no-store", response.headers["Cache-Control"]
      assert_includes response.headers["WWW-Authenticate"], "invalid_token"
    end
  ensure
    original&.each { |name, value| value.nil? ? ENV.delete(name) : ENV[name] = value }
  end

  test "verified subject maps to an enrolled user without trusting email or role claims" do
    with_identity_provider do
      user = create_identity_user
      token = signed_token({ "email" => "owner@example.test", "email_verified" => true, "role" => "owner" })
      get "/api/v1/me", headers: { "Authorization" => "Bearer #{token}" }
      assert_response :success
      assert_equal "cosigna.identity/v1", response.parsed_body["schema"]
      assert_equal({ "id" => user.public_id, "display_name" => user.display_name }, response.parsed_body["user"])
      refute_includes response.body, "subject-a"
      refute_includes response.body, token
      get "/api/v1/me", params: { access_token: token }
      assert_response :unauthorized
      refute_includes response.body, token
      before = ApplicationUser.count
      get "/api/v1/me", headers: { "Authorization" => "Bearer #{signed_token({ "sub" => "unregistered", "email" => "owner@example.test" })}" }
      assert_response :unauthorized
      assert_equal before, ApplicationUser.count
    end
  end

  test "configuration and identity-store errors return safe service codes" do
    with_identity_provider do |path|
      user = create_identity_user
      headers = { "Authorization" => "Bearer #{signed_token}" }
      ExternalIdentity.stub :active, -> { raise ActiveRecord::StatementInvalid, "private-database-detail" } do
        get "/api/v1/me", headers: headers
        assert_response :service_unavailable
        assert_equal "identity_unavailable", response.parsed_body["error"]
        refute_includes response.body, "private-database-detail"
      end
      assert user.active?
      File.write(path, "private-malformed-configuration")
      get "/api/v1/me", headers: headers
      assert_response :service_unavailable
      assert_equal "identity_configuration_invalid", response.parsed_body["error"]
      refute_includes response.body, path
      refute_includes response.body, "private-malformed-configuration"
    end
  end

  test "organization access is derived from membership and unrelated or nonexistent ids look alike" do
    with_identity_provider do
      alice = create_identity_user
      bob = create_identity_user(subject: "subject-b")
      first = Organization.create!(name: "First synthetic company")
      second = Organization.create!(name: "Second synthetic company")
      OrganizationMembership.create!(application_user: alice, organization: first, role: "viewer")
      OrganizationMembership.create!(application_user: bob, organization: second, role: "owner")
      headers = { "Authorization" => "Bearer #{signed_token({ "role" => "owner", "organization_id" => second.public_id })}" }
      get "/api/v1/organizations", params: { organization_id: second.public_id, owner_id: bob.public_id }, headers: headers
      assert_response :success
      assert_equal [ first.public_id ], response.parsed_body["organizations"].map { |org| org["id"] }
      assert_equal "viewer", response.parsed_body["organizations"].first["role"]
      get "/api/v1/organizations/#{first.public_id}", headers: headers
      assert_response :success
      get "/api/v1/organizations/#{second.public_id}", headers: headers
      assert_response :not_found
      denied = response.parsed_body
      get "/api/v1/organizations/org_#{'0' * 32}", headers: headers
      assert_response :not_found
      assert_equal denied, response.parsed_body
      refute_includes response.body, second.name
    end
  end

  test "membership identity user and organization revocations take effect on new requests" do
    with_identity_provider do
      user = create_identity_user
      org = Organization.create!(name: "Revocable company")
      membership = OrganizationMembership.create!(application_user: user, organization: org, role: "owner")
      headers = { "Authorization" => "Bearer #{signed_token}" }
      get "/api/v1/organizations/#{org.public_id}", headers: headers
      assert_response :success
      membership.update!(revoked_at: Time.current)
      get "/api/v1/organizations/#{org.public_id}", headers: headers
      assert_response :not_found
      membership.update!(revoked_at: nil)
      org.update!(active: false)
      get "/api/v1/organizations", headers: headers
      assert_equal [], response.parsed_body["organizations"]
      user.external_identities.first.update!(revoked_at: Time.current)
      get "/api/v1/me", headers: headers
      assert_response :unauthorized
      user.external_identities.first.update!(revoked_at: nil)
      user.update!(active: false)
      get "/api/v1/me", headers: headers
      assert_response :unauthorized
    end
  end

  test "organization listing is bounded and paginated within the authenticated user" do
    with_identity_provider do
      user = create_identity_user
      51.times do |index|
        org = Organization.create!(name: "Synthetic organization #{index}")
        OrganizationMembership.create!(application_user: user, organization: org, role: "member")
      end
      headers = { "Authorization" => "Bearer #{signed_token}" }
      get "/api/v1/organizations", params: { limit: 10_000 }, headers: headers
      assert_response :success
      first = response.parsed_body
      assert_equal 50, first["organizations"].length
      assert first["next_cursor"]
      get "/api/v1/organizations", params: { after: first["next_cursor"] }, headers: headers
      assert_response :success
      assert_equal 1, response.parsed_body["organizations"].length
      assert_nil response.parsed_body["next_cursor"]
      refute_includes first["organizations"].map { |org| org["id"] }, response.parsed_body["organizations"].first["id"]
      get "/api/v1/organizations", params: { after: "untrusted-cursor-value" }, headers: headers
      assert_response :bad_request
      refute_includes response.body, "untrusted-cursor-value"
    end
  end

  test "insufficient scope is distinct and read identity never enables enrollment or payment writes" do
    with_identity_provider do
      create_identity_user
      get "/api/v1/me", headers: { "Authorization" => "Bearer #{signed_token({ "scope" => "openid" })}" }
      assert_response :forbidden
      assert_equal "insufficient_scope", response.parsed_body["error"]
      headers = { "Authorization" => "Bearer #{signed_token}" }
      [ "/api/v1/users", "/api/v1/organizations", "/api/v1/tasks/example/approve", "/gateway/v1/operations" ].each do |path|
        post path, params: { owner_id: "untrusted-owner", role: "owner" }, headers: headers
        assert_response :not_found
      end
    end
  end
end
