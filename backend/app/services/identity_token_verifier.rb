require "jwt"
require "uri"
require "pathname"

# A resource-server boundary for a configured issuer's JWT access tokens (RFC 9068).
# Trust comes only from operator configuration. No discovery, URL fetch or token issuance.
class IdentityTokenVerifier
  class Unconfigured < StandardError; end
  class InvalidConfiguration < StandardError; end
  class InvalidToken < StandardError; end
  class InsufficientScope < StandardError; end

  Subject = Data.define(:issuer, :subject)
  MAX_TOKEN_BYTES = 16_384
  MAX_JWKS_BYTES = 65_536
  MAX_LIFETIME = 3600
  REQUIRED_CLAIMS = %w[iss sub aud exp iat jti client_id scope].freeze
  CONFIG_NAMES = %w[COSIGNA_IDENTITY_ISSUER COSIGNA_IDENTITY_AUDIENCE
    COSIGNA_IDENTITY_CLIENT_IDS COSIGNA_IDENTITY_JWKS_PATH].freeze

  def self.from_environment(env = ENV)
    values = CONFIG_NAMES.map { |name| env[name] }
    raise Unconfigured if values.all?(&:blank?)
    raise InvalidConfiguration if values.any?(&:blank?)
    path = values.last
    raise InvalidConfiguration unless Pathname.new(path).absolute? && File.file?(path)
    text = File.open(path, "rb") { |file| file.read(MAX_JWKS_BYTES + 1) } || ""
    raise InvalidConfiguration if text.bytesize > MAX_JWKS_BYTES
    new(issuer: values[0], audience: values[1], client_ids: JSON.parse(values[2]), jwks: JSON.parse(text))
  rescue JSON::ParserError, IOError, SystemCallError, ArgumentError
    raise InvalidConfiguration
  end

  def initialize(issuer:, audience:, client_ids:, jwks:)
    uri = URI.parse(issuer)
    unless uri.is_a?(URI::HTTPS) && uri.host.present? && uri.userinfo.nil? && uri.query.nil? && uri.fragment.nil? &&
        bounded_text?(issuer, 2048) && bounded_text?(audience, 255) &&
        client_ids.is_a?(Array) && (1..10).cover?(client_ids.length) &&
        client_ids.uniq.length == client_ids.length && client_ids.all? { |id| bounded_text?(id, 255) }
      raise InvalidConfiguration
    end
    @issuer, @audience, @client_ids = issuer.dup.freeze, audience.dup.freeze, client_ids.map { |id| id.dup.freeze }.freeze
    keys = jwks.is_a?(Hash) ? jwks["keys"] : nil
    raise InvalidConfiguration unless keys.is_a?(Array) && (1..10).cover?(keys.length)
    @keys = {}
    keys.each do |entry|
      unless entry.is_a?(Hash) && entry["kty"] == "RSA" &&
          entry["kid"].is_a?(String) && entry["kid"].match?(/\A[A-Za-z0-9._-]{1,128}\z/) &&
          !@keys.key?(entry["kid"]) && [ nil, "RS256" ].include?(entry["alg"]) &&
          [ nil, "sig" ].include?(entry["use"]) && [ nil, [ "verify" ] ].include?(entry["key_ops"]) &&
          (entry.keys & %w[d p q dp dq qi oth k]).empty? &&
          %w[n e].all? { |name| entry[name].is_a?(String) && entry[name].match?(/\A[A-Za-z0-9_-]{1,1024}\z/) }
        raise InvalidConfiguration
      end
      key = JWT::JWK.import(entry).verify_key
      raise InvalidConfiguration unless key.is_a?(OpenSSL::PKey::RSA) && !key.private? &&
        (2048..4096).cover?(key.n.num_bits) && key.e.to_i == 65_537
      @keys[entry["kid"].dup.freeze] = key
    end
    @keys.freeze
  rescue URI::InvalidURIError, ArgumentError, TypeError, JWT::JWKError, OpenSSL::PKey::PKeyError
    raise InvalidConfiguration
  end

  def call(authorization)
    unless authorization.is_a?(String) && authorization.bytesize <= MAX_TOKEN_BYTES &&
        authorization.match?(/\ABearer [A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\z/)
      raise InvalidToken
    end
    token = authorization.delete_prefix("Bearer ")
    claims, = JWT.decode(token, nil, true,
      algorithm: "RS256", verify_iss: true, iss: @issuer, verify_aud: true, aud: @audience,
      verify_expiration: true, verify_not_before: true, verify_iat: true, required_claims: REQUIRED_CLAIMS
    ) do |header, unverified_claims|
      # Type-check before the library's claim validators; no unverified claim grants access.
      unless unverified_claims.is_a?(Hash) && header.is_a?(Hash) && (header.keys - %w[alg kid typ]).empty? && header["alg"] == "RS256" &&
          header["typ"].is_a?(String) && %w[at+jwt application/at+jwt].include?(header["typ"].downcase)
        raise InvalidToken
      end
      @keys.fetch(header["kid"]) { raise InvalidToken }
    end
    now = Time.now.to_i
    unless claims.is_a?(Hash) && claims["iss"] == @issuer &&
        bounded_text?(claims["sub"], 255) && bounded_text?(claims["jti"], 255) &&
        @client_ids.include?(claims["client_id"]) &&
        [ claims["exp"], claims["iat"] ].all? { |value| value.is_a?(Integer) } &&
        claims["iat"] <= now && claims["exp"] > now && claims["exp"] > claims["iat"] &&
        claims["exp"] - claims["iat"] <= MAX_LIFETIME &&
        (!claims.key?("nbf") || (claims["nbf"].is_a?(Integer) && claims["nbf"] <= now && claims["nbf"] < claims["exp"])) &&
        valid_audience?(claims["aud"]) && bounded_text?(claims["scope"], 1024, spaces: true)
      raise InvalidToken
    end
    raise InsufficientScope unless claims["scope"].split(" ").include?("cosigna:read")
    Subject.new(issuer: @issuer, subject: claims["sub"].dup.freeze)
  rescue JWT::DecodeError, ArgumentError, TypeError
    raise InvalidToken
  end

  private

  def bounded_text?(value, max, spaces: false)
    value.is_a?(String) && value.valid_encoding? && !value.empty? && value.bytesize <= max &&
      !value.match?(spaces ? /[\x00-\x1f\x7f]/ : /[\x00-\x20\x7f]/)
  end

  def valid_audience?(value)
    value == @audience || (value.is_a?(Array) && (1..10).cover?(value.length) &&
      value.all? { |aud| bounded_text?(aud, 255) } && value.include?(@audience))
  end
end
