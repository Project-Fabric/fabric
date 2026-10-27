module Api
  module V1
    class PlatformController < ApplicationController
      def show
        render json: {
          schema: "cosigna.backend/v2",
          product: "Cosigna",
          phase: "development_identity",
          application_backend: "rails",
          transaction_authority: "rust_gateway",
          customer_authentication: "external_access_token_verification",
          customer_enrollment: "operator_only",
          organization_access: "membership_scoped_reads",
          gateway_owner_mapping: "not_implemented",
          transaction_writes: "not_exposed"
        }
      end
    end
  end
end
