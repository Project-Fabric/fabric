class AuthenticatedController < ApplicationController
  before_action :authenticate_application_user!
  rescue_from IdentityTokenVerifier::Unconfigured, with: -> { identity_error("identity_unconfigured", :service_unavailable) }
  rescue_from IdentityTokenVerifier::InvalidConfiguration, with: -> { identity_error("identity_configuration_invalid", :service_unavailable) }
  rescue_from IdentityTokenVerifier::InvalidToken, with: :invalid_token
  rescue_from IdentityTokenVerifier::InsufficientScope, with: :insufficient_scope
  rescue_from ActiveRecord::ConnectionNotEstablished, ActiveRecord::StatementInvalid,
    with: -> { identity_error("identity_unavailable", :service_unavailable) }

  private

  def authenticate_application_user!
    subject = IdentityTokenVerifier.from_environment.call(request.headers["Authorization"])
    identity = ExternalIdentity.active.includes(:application_user).find_by(issuer: subject.issuer, subject: subject.subject)
    @current_user = identity&.application_user
    raise IdentityTokenVerifier::InvalidToken unless @current_user&.active?
  end

  def current_memberships
    @current_user.organization_memberships.active.joins(:organization).merge(Organization.active)
  end

  def invalid_token
    response.headers["WWW-Authenticate"] = 'Bearer error="invalid_token"'
    identity_error("invalid_token", :unauthorized)
  end

  def insufficient_scope
    response.headers["WWW-Authenticate"] = 'Bearer error="insufficient_scope", scope="cosigna:read"'
    identity_error("insufficient_scope", :forbidden)
  end

  def identity_error(code, status)
    render json: { schema: "cosigna.identity/v1", error: code }, status: status
  end
end
