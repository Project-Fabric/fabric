module Api
  module V1
    class IdentitiesController < AuthenticatedController
      def show
        render json: { schema: "cosigna.identity/v1", user: {
          id: @current_user.public_id, display_name: @current_user.display_name
        } }
      end
    end
  end
end
