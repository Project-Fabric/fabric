module Api
  module V1
    class OrganizationsController < AuthenticatedController
      PAGE_SIZE = 50

      def index
        after = params[:after]
        unless after.nil? || valid_id?(after)
          return identity_error("invalid_cursor", :bad_request)
        end
        rows = current_memberships.includes(:organization).order("organizations.public_id")
        rows = rows.where("organizations.public_id > ?", after) if after
        page = rows.limit(PAGE_SIZE + 1).to_a
        more = page.length > PAGE_SIZE
        page = page.first(PAGE_SIZE)
        render json: { schema: "cosigna.identity/v1", organizations: page.map { |row| view(row) },
          next_cursor: more ? page.last.organization.public_id : nil }
      end

      def show
        row = current_memberships.includes(:organization).find_by(organizations: { public_id: params[:id] }) if valid_id?(params[:id])
        return identity_error("not_found", :not_found) unless row
        render json: { schema: "cosigna.identity/v1", organization: view(row) }
      end

      private

      def valid_id?(id)
        id.is_a?(String) && id.match?(/\Aorg_[0-9a-f]{32}\z/)
      end

      def view(membership)
        { id: membership.organization.public_id, name: membership.organization.name, role: membership.role }
      end
    end
  end
end
