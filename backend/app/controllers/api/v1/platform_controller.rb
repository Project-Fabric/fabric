module Api
  module V1
    class PlatformController < ApplicationController
      def show
        render json: {
          schema: "cosigna.backend/v1",
          product: "Cosigna",
          phase: "development_foundation",
          application_backend: "rails",
          transaction_authority: "rust_gateway",
          customer_authentication: "not_implemented",
          transaction_writes: "not_exposed"
        }
      end
    end
  end
end
