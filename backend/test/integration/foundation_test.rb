require "test_helper"

class FoundationTest < ActionDispatch::IntegrationTest
  setup { host! "127.0.0.1" }

  test "application health and versioned product boundary are available without credentials" do
    get "/up"
    assert_response :success
    get "/api/v1/platform"
    assert_response :success
    assert_equal "cosigna.backend/v2", response.parsed_body["schema"]
    assert_equal "rust_gateway", response.parsed_body["transaction_authority"]
    assert_equal "external_access_token_verification", response.parsed_body["customer_authentication"]
    assert_equal "operator_only", response.parsed_body["customer_enrollment"]
    assert_equal "not_implemented", response.parsed_body["gateway_owner_mapping"]
    assert_equal "not_exposed", response.parsed_body["transaction_writes"]
    assert_equal "no-store", response.headers["Cache-Control"]
  end

  test "untrusted hosts and transaction write routes are refused" do
    host! "untrusted.example"
    get "/api/v1/platform"
    assert_response :forbidden
    host! "127.0.0.1"
    post "/api/v1/platform"
    assert_response :not_found
    post "/api/v1/tasks/example/approve", params: { quote_digest: "untrusted" }
    assert_response :not_found
  end

  test "unconfigured gateway is reported honestly" do
    client = Object.new
    def client.call = raise GatewayHealthClient::Unconfigured
    GatewayHealthClient.stub :new, -> { client } do
      get "/api/v1/gateway/health"
      assert_response :service_unavailable
      assert_equal "gateway_unconfigured", response.parsed_body["error"]
    end
  end

  test "a verified sandbox gateway is reported without forwarding request parameters" do
    client = Object.new
    def client.call = { schema: "cosigna.agent/v1", status: "ok", simulated_provider: true }
    GatewayHealthClient.stub :new, -> { client } do
      get "/api/v1/gateway/health", params: { url: "https://untrusted.example" }
      assert_response :success
      assert_equal true, response.parsed_body.dig("gateway", "simulated_provider")
    end
  end

  test "upstream failures do not leak exception details" do
    client = Object.new
    def client.call = raise GatewayHealthClient::Unavailable, "private-upstream-detail"
    GatewayHealthClient.stub :new, -> { client } do
      get "/api/v1/gateway/health"
      assert_response :service_unavailable
      assert_equal "gateway_unavailable", response.parsed_body["error"]
      refute_includes response.body, "private-upstream-detail"
    end
  end
end
