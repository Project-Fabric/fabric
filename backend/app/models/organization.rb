class Organization < ApplicationRecord
  has_many :organization_memberships, dependent: :restrict_with_exception
  scope :active, -> { where(active: true) }

  before_validation(on: :create) { self.public_id ||= "org_#{SecureRandom.hex(16)}" }
  attr_readonly :public_id
  validates :public_id, format: { with: /\Aorg_[0-9a-f]{32}\z/ }, uniqueness: true
  validates :name, presence: true, length: { maximum: 100 }
  validates :active, inclusion: { in: [ true, false ] }
end
