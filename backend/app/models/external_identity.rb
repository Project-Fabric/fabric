class ExternalIdentity < ApplicationRecord
  belongs_to :application_user
  scope :active, -> { where(revoked_at: nil) }
  self.filter_attributes += [ :issuer, :subject ]

  validates :issuer, presence: true, length: { maximum: 2048 }
  validates :subject, presence: true, length: { maximum: 255 }, uniqueness: { scope: :issuer }
end
