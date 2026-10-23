require "net/http"
require "json"

# Only probes the existing public development health route. No owner or payment credentials.
class GatewayHealthClient
  class Unconfigured < StandardError; end
  class InvalidConfiguration < StandardError; end
  class Unverified < StandardError; end
  class Unavailable < StandardError; end

  MAX_BYTES = 4096
  ALLOWED_HOSTS = [ "127.0.0.1", "host.docker.internal" ].freeze

  def initialize(host: ENV.fetch("COSIGNA_GATEWAY_HEALTH_HOST", "127.0.0.1"), port: ENV["COSIGNA_GATEWAY_HEALTH_PORT"])
    @host = host
    @port = port
  end

  def call
    raise Unconfigured if @port.nil? || @port.empty?
    unless ALLOWED_HOSTS.include?(@host) && @port.match?(/\A[0-9]{4,5}\z/) && (1024..65535).cover?(@port.to_i)
      raise InvalidConfiguration
    end

    # Disable environment proxy discovery; this is a narrowly scoped local development probe.
    http = Net::HTTP.new(@host, @port.to_i, nil)
    http.open_timeout = 1
    http.read_timeout = 1
    http.write_timeout = 1
    http.max_retries = 0
    request = Net::HTTP::Get.new("/v1/health")
    request["Host"] = "127.0.0.1:#{@port}"
    request["Accept"] = "application/json"
    body = +""
    http.request(request) do |response|
      raise Unverified unless response.code == "200"
      response.read_body do |chunk|
        raise Unverified if body.bytesize + chunk.bytesize > MAX_BYTES
        body << chunk
      end
    end
    value = JSON.parse(body)
    unless value.is_a?(Hash) && value["schema"] == "cosigna.agent/v1" && value["status"] == "ok" && value["simulated_provider"] == true
      raise Unverified
    end
    # Return a whitelist, never an upstream body, address, token, or exception message.
    { schema: value["schema"], status: "ok", simulated_provider: true }
  rescue JSON::ParserError
    raise Unverified
  rescue IOError, SystemCallError, SocketError, Timeout::Error, Net::HTTPBadResponse, Net::ProtocolError
    raise Unavailable
  end
end
