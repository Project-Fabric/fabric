require "test_helper"
require "net/http"
require_relative "../support/synthetic_identity"

class IdentityTokenVerifierTest < ActiveSupport::TestCase
  include SyntheticIdentity
  setup { travel_to Time.utc(2026, 10, 27, 12) }
  teardown { travel_back }

  test "signed access tokens return only the verified issuer and subject" do
    subject = identity_verifier.call("Bearer #{signed_token}")
    assert_equal SyntheticIdentity::ISSUER, subject.issuer
    assert_equal "subject-a", subject.subject
    assert subject.frozen?
    assert subject.subject.frozen?
    assert_equal %i[issuer subject], subject.members
    token = signed_token({ "aud" => [ "another-api", SyntheticIdentity::AUDIENCE ] }, header: { typ: "application/at+JWT" })
    assert_equal "subject-a", identity_verifier.call("Bearer #{token}").subject
  end

  test "tampering wrong keys and algorithm or ID-token confusion fail closed" do
    valid = signed_token
    parts = valid.split(".")
    parts[1] = Base64.urlsafe_encode64(JSON.generate("sub" => "attacker"), padding: false)
    bad_tokens = [ parts.join("."), signed_token(key: SyntheticIdentity::OTHER_KEY),
      signed_token(header: { typ: "JWT" }), signed_token(header: { alg: "none" }),
      signed_token(header: { alg: "HS256" }),
      signed_token(header: { kid: "unknown" }), signed_token(header: { crit: [ "b64" ] }),
      signed_token(header: { jku: "https://untrusted.example/jwks" }),
      signed_token(header: { jwk: public_jwk(SyntheticIdentity::OTHER_KEY) }),
      signed_token(key: SyntheticIdentity::KEY.public_key.to_pem, algorithm: "HS256") ]
    bad_tokens.each do |token|
      assert_raises(IdentityTokenVerifier::InvalidToken) { identity_verifier.call("Bearer #{token}") }
    end
  end

  test "issuer audience client lifetime and required claims are verified" do
    now = Time.now.to_i
    bad_claims = [ { "iss" => "https://other.example/" }, { "aud" => "wrong-api" },
      { "client_id" => "unapproved-client" }, { "sub" => "" }, { "jti" => "" },
      { "exp" => now - 1 }, { "iat" => now + 100 }, { "nbf" => now + 100 },
      { "nbf" => "0" }, { "nbf" => nil }, { "exp" => (now + 300).to_s }, { "exp" => now + 300.5 },
      { "iat" => now.to_f }, { "exp" => now + 3601 }, { "aud" => [ SyntheticIdentity::AUDIENCE, 7 ] } ]
    bad_claims.each do |claims|
      assert_raises(IdentityTokenVerifier::InvalidToken, claims.keys.join(",")) do
        identity_verifier.call("Bearer #{signed_token(claims)}")
      end
    end
    IdentityTokenVerifier::REQUIRED_CLAIMS.each do |claim|
      assert_raises(IdentityTokenVerifier::InvalidToken, claim) do
        identity_verifier.call("Bearer #{signed_token(omit: [ claim ])}")
      end
    end
  end

  test "read scope is mandatory and bearer parsing is bounded" do
    assert_raises(IdentityTokenVerifier::InsufficientScope) do
      identity_verifier.call("Bearer #{signed_token({ "scope" => "openid profile" })}")
    end
    [ nil, "", "Basic secret", "Bearer #{signed_token},other", "Bearer #{'x' * 16_384}" ].each do |header|
      assert_raises(IdentityTokenVerifier::InvalidToken) { identity_verifier.call(header) }
    end
  end

  test "public key configuration rejects private weak duplicate or incompatible keys" do
    private_jwk = JWT::JWK.new(SyntheticIdentity::KEY, kid: "private").export(include_private: true).transform_keys(&:to_s)
    weak_jwk = public_jwk(OpenSSL::PKey::RSA.generate(1024), "weak")
    bad_sets = [ [], [ public_jwk, public_jwk ], [ private_jwk ], [ weak_jwk ],
      [ public_jwk.merge("use" => "enc") ], [ public_jwk.merge("key_ops" => [ "sign" ]) ],
      [ public_jwk.merge("alg" => "HS256") ], [ public_jwk.merge("n" => "malformed!") ],
      [ public_jwk.merge("kid" => "") ] ]
    bad_sets.each do |keys|
      assert_raises(IdentityTokenVerifier::InvalidConfiguration) { identity_verifier(jwks: { "keys" => keys }) }
    end
    [ { issuer: "http://identity.example/" }, { issuer: "https://user:password@identity.example/" },
      { audience: "" }, { client_ids: [] } ].each do |config|
      assert_raises(IdentityTokenVerifier::InvalidConfiguration) { identity_verifier(**config) }
    end
  end

  test "environment configuration is optional but must be complete bounded and local" do
    assert_raises(IdentityTokenVerifier::Unconfigured) { IdentityTokenVerifier.from_environment({}) }
    assert_raises(IdentityTokenVerifier::InvalidConfiguration) do
      IdentityTokenVerifier.from_environment("COSIGNA_IDENTITY_ISSUER" => SyntheticIdentity::ISSUER)
    end
    with_identity_provider do |path|
      assert_equal "subject-a", IdentityTokenVerifier.from_environment.call("Bearer #{signed_token}").subject
      File.write(path, "")
      assert_raises(IdentityTokenVerifier::InvalidConfiguration) { IdentityTokenVerifier.from_environment }
      File.write(path, "x" * (IdentityTokenVerifier::MAX_JWKS_BYTES + 1))
      assert_raises(IdentityTokenVerifier::InvalidConfiguration) { IdentityTokenVerifier.from_environment }
    end
  end

  test "key retirement takes effect on the next verifier load without discovery" do
    with_identity_provider do |path|
      File.write(path, JSON.generate("keys" => [ public_jwk, public_jwk(SyntheticIdentity::OTHER_KEY, "next-key") ]))
      assert_equal "subject-a", IdentityTokenVerifier.from_environment.call("Bearer #{signed_token}").subject
      File.write(path, JSON.generate("keys" => [ public_jwk(SyntheticIdentity::OTHER_KEY, "next-key") ]))
      assert_raises(IdentityTokenVerifier::InvalidToken) { IdentityTokenVerifier.from_environment.call("Bearer #{signed_token}") }
      replacement = signed_token(key: SyntheticIdentity::OTHER_KEY, header: { kid: "next-key" })
      assert_equal "subject-a", IdentityTokenVerifier.from_environment.call("Bearer #{replacement}").subject
    end
  end

  test "unknown keys never trigger remote discovery" do
    Net::HTTP.stub :new, ->(*) { flunk "Token verification must not start HTTP" } do
      assert_raises(IdentityTokenVerifier::InvalidToken) do
        identity_verifier.call("Bearer #{signed_token(header: { kid: "missing-key" })}")
      end
      assert_raises(IdentityTokenVerifier::InvalidToken) do
        identity_verifier.call("Bearer #{signed_token(header: { jku: "https://untrusted.example/keys" })}")
      end
    end
  end

  test "malformed decoded JSON shapes fail as invalid tokens" do
    [ [], nil, 123, "not-an-object" ].each do |payload|
      header = Base64.urlsafe_encode64(JSON.generate(alg: "RS256", kid: "test-key", typ: "at+jwt"), padding: false)
      body = Base64.urlsafe_encode64(JSON.generate(payload), padding: false)
      input = "#{header}.#{body}"
      signature = SyntheticIdentity::KEY.sign(OpenSSL::Digest.new("SHA256"), input)
      token = "#{input}.#{Base64.urlsafe_encode64(signature, padding: false)}"
      assert_raises(IdentityTokenVerifier::InvalidToken) { identity_verifier.call("Bearer #{token}") }
    end
    [ [], nil, 123, "not-an-object" ].each do |value|
      header = Base64.urlsafe_encode64(JSON.generate(value), padding: false)
      token = "#{header}.#{signed_token.split('.')[1]}.eA"
      assert_raises(IdentityTokenVerifier::InvalidToken) { identity_verifier.call("Bearer #{token}") }
    end
  end
end
