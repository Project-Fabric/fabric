require "test_helper"

class ApplicationIdentityTest < ActiveSupport::TestCase
  test "identity pairs and memberships remain unique at the database boundary" do
    user = ApplicationUser.create!(display_name: "Synthetic user")
    org = Organization.create!(name: "Synthetic company")
    identity = ExternalIdentity.create!(application_user: user, issuer: "https://issuer.example/", subject: "synthetic-subject")
    membership = OrganizationMembership.create!(application_user: user, organization: org, role: "viewer")
    assert_raises(ActiveRecord::RecordNotUnique) do
      ExternalIdentity.insert_all!([ identity.attributes.except("id") ])
    end
    assert_raises(ActiveRecord::RecordNotUnique) do
      OrganizationMembership.insert_all!([ membership.attributes.except("id") ])
    end
    assert_raises(ActiveRecord::StatementInvalid) do
      OrganizationMembership.where(id: membership.id).update_all(role: "super_owner")
    end
  end

  test "memberships require existing records and public identifiers are generated and immutable" do
    user = ApplicationUser.create!(display_name: "Synthetic user")
    org = Organization.create!(name: "Synthetic company")
    assert_match(/\Ausr_[0-9a-f]{32}\z/, user.public_id)
    assert_match(/\Aorg_[0-9a-f]{32}\z/, org.public_id)
    assert_raises(ActiveRecord::ReadonlyAttributeError) { user.update!(public_id: "usr_#{'0' * 32}") }
    assert_raises(ActiveRecord::InvalidForeignKey) do
      OrganizationMembership.insert_all!([ { application_user_id: user.id + 9999,
        organization_id: org.id, role: "viewer", created_at: Time.current, updated_at: Time.current } ])
    end
  end
end
