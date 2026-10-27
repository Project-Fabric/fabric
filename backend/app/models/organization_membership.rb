class OrganizationMembership < ApplicationRecord
  ROLES = %w[owner admin member viewer].freeze
  belongs_to :application_user
  belongs_to :organization
  scope :active, -> { where(revoked_at: nil) }

  validates :application_user_id, uniqueness: { scope: :organization_id }
  validates :role, inclusion: { in: ROLES }
end
