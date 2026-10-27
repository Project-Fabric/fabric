class ApplicationUser < ApplicationRecord
  has_many :external_identities, dependent: :restrict_with_exception
  has_many :organization_memberships, dependent: :restrict_with_exception

  before_validation(on: :create) { self.public_id ||= "usr_#{SecureRandom.hex(16)}" }
  attr_readonly :public_id
  validates :public_id, format: { with: /\Ausr_[0-9a-f]{32}\z/ }, uniqueness: true
  validates :display_name, presence: true, length: { maximum: 100 }
  validates :active, inclusion: { in: [ true, false ] }
end
