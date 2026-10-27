require "tmpdir"
require "jwt"

# Private signing keys live only in test-process memory; public keys go to a temporary file.
# This is not an application token issuer or a default account.
module SyntheticIdentity
  ISSUER = "https://identity.example.test/".freeze
  AUDIENCE = "urn:cosigna:identity-tests".freeze
  CLIENT = "synthetic-client".freeze
  KEY = OpenSSL::PKey::RSA.generate(2048)
  OTHER_KEY = OpenSSL::PKey::RSA.generate(2048)

  def public_jwk(key = KEY, kid = "test-key")
    JWT::JWK.new(key.public_key, kid: kid, alg: "RS256", use: "sig").export.transform_keys(&:to_s)
  end

  def identity_verifier(jwks: { "keys" => [ public_jwk ] }, **overrides)
    IdentityTokenVerifier.new(**{ issuer: ISSUER, audience: AUDIENCE, client_ids: [ CLIENT ], jwks: jwks }.merge(overrides))
  end

  def signed_token(overrides = {}, omit: [], header: {}, key: KEY, algorithm: "RS256")
    now = Time.now.to_i
    claims = { "iss" => ISSUER, "sub" => "subject-a", "aud" => AUDIENCE,
      "iat" => now, "exp" => now + 300, "jti" => "synthetic-token-id",
      "client_id" => CLIENT, "scope" => "cosigna:read" }.merge(overrides).except(*omit)
    headers = { "typ" => "at+jwt", "kid" => "test-key", "alg" => algorithm }.merge(header.transform_keys(&:to_s))
    return JWT.encode(claims, key, algorithm, headers) unless algorithm == "RS256"
    # Sign test bytes directly so malformed registered-claim values reach the verifier;
    # ruby-jwt's issuer-side encoder correctly refuses some of these attack fixtures.
    parts = [ headers, claims ].map { |value| Base64.urlsafe_encode64(JSON.generate(value), padding: false) }
    input = parts.join(".")
    signature = key.sign(OpenSSL::Digest.new("SHA256"), input)
    input + "." + Base64.urlsafe_encode64(signature, padding: false)
  end

  def with_identity_provider
    original = IdentityTokenVerifier::CONFIG_NAMES.to_h { |name| [ name, ENV[name] ] }
    Dir.mktmpdir("cosigna-identity-tests") do |directory|
      path = File.join(directory, "jwks.json")
      File.write(path, JSON.generate("keys" => [ public_jwk ]))
      ENV["COSIGNA_IDENTITY_ISSUER"] = ISSUER
      ENV["COSIGNA_IDENTITY_AUDIENCE"] = AUDIENCE
      ENV["COSIGNA_IDENTITY_CLIENT_IDS"] = JSON.generate([ CLIENT ])
      ENV["COSIGNA_IDENTITY_JWKS_PATH"] = path
      yield path
    end
  ensure
    original&.each { |name, value| value.nil? ? ENV.delete(name) : ENV[name] = value }
  end

  def create_identity_user(subject: "subject-a", issuer: ISSUER)
    user = ApplicationUser.create!(display_name: "Synthetic user")
    ExternalIdentity.create!(application_user: user, issuer: issuer, subject: subject)
    user
  end
end
