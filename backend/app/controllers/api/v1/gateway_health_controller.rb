module Api
  module V1
    class GatewayHealthController < ApplicationController
      def show
        render json: { schema: "cosigna.backend/v1", gateway: GatewayHealthClient.new.call }
      rescue GatewayHealthClient::Unconfigured
        unavailable("gateway_unconfigured")
      rescue GatewayHealthClient::InvalidConfiguration
        unavailable("gateway_configuration_invalid")
      rescue GatewayHealthClient::Unverified
        unavailable("gateway_unverified")
      rescue GatewayHealthClient::Unavailable
        unavailable("gateway_unavailable")
      end

      private

      def unavailable(code)
        render json: { schema: "cosigna.backend/v1", error: code }, status: :service_unavailable
      end
    end
  end
end
