require "test_helper"
require "socket"

class GatewayHealthClientTest < ActiveSupport::TestCase
  HEALTH = { schema: "cosigna.agent/v1", status: "ok", simulated_provider: true }.to_json.freeze

  def with_gateway(body: HEALTH, status: "200 OK", headers: "")
    server = TCPServer.new("127.0.0.1", 0)
    port = server.addr[1]
    requests = Queue.new
    thread = Thread.new do
      socket = server.accept
      request = +""
      request << socket.readpartial(1024) until request.include?("\r\n\r\n")
      requests << request
      socket.write("HTTP/1.1 #{status}\r\nContent-Type: application/json\r\nContent-Length: #{body.bytesize}\r\n#{headers}Connection: close\r\n\r\n#{body}")
    ensure
      socket&.close
    end
    yield GatewayHealthClient.new(host: "127.0.0.1", port: port.to_s), requests
  ensure
    thread&.join(2)
    thread&.kill
    server&.close
  end

  test "only the fixed health path is requested with the gateway's expected Host and no authorization" do
    with_gateway(body: JSON.parse(HEALTH).merge("extra" => "private-upstream-detail").to_json) do |client, requests|
      assert_equal({ schema: "cosigna.agent/v1", status: "ok", simulated_provider: true }, client.call)
      request = requests.pop
      assert_match(/\AGET \/v1\/health HTTP\/1.1\r\n/, request)
      assert_match(/\r\nHost: 127\.0\.0\.1:[0-9]+\r\n/i, request)
      refute_match(/Authorization:/i, request)
    end
  end

  test "missing configuration and arbitrary destinations are rejected before I/O" do
    assert_raises(GatewayHealthClient::Unconfigured) { GatewayHealthClient.new(port: nil).call }
    [ [ "example.com", "3100" ], [ "127.0.0.1", "80" ], [ "127.0.0.1", "3100/path" ] ].each do |host, port|
      assert_raises(GatewayHealthClient::InvalidConfiguration) { GatewayHealthClient.new(host: host, port: port).call }
    end
  end

  test "redirects are never followed" do
    with_gateway(status: "302 Found", headers: "Location: https://untrusted.example\r\n") do |client, _|
      assert_raises(GatewayHealthClient::Unverified) { client.call }
    end
  end

  test "a refused local connection is an unavailable gateway" do
    listener = TCPServer.new("127.0.0.1", 0)
    port = listener.addr[1].to_s
    listener.close
    assert_raises(GatewayHealthClient::Unavailable) { GatewayHealthClient.new(port: port).call }
  end

  test "invalid, oversized or non-sandbox replies cannot establish gateway readiness" do
    [ "not-json", "[]", "x" * 4097, { schema: "cosigna.agent/v1", status: "ok", simulated_provider: false }.to_json ].each do |body|
      with_gateway(body: body) { |client, _| assert_raises(GatewayHealthClient::Unverified) { client.call } }
    end
  end
end
