class CreateApplicationIdentity < ActiveRecord::Migration[8.1]
  def change
    create_table :application_users do |t|
      t.string :public_id, null: false
      t.string :display_name, null: false
      t.boolean :active, null: false, default: true
      t.timestamps
    end
    add_index :application_users, :public_id, unique: true

    create_table :external_identities do |t|
      t.references :application_user, null: false, foreign_key: true
      t.string :issuer, null: false
      t.string :subject, null: false
      t.datetime :revoked_at
      t.timestamps
    end
    add_index :external_identities, [ :issuer, :subject ], unique: true

    create_table :organizations do |t|
      t.string :public_id, null: false
      t.string :name, null: false
      t.boolean :active, null: false, default: true
      t.timestamps
    end
    add_index :organizations, :public_id, unique: true

    create_table :organization_memberships do |t|
      t.references :application_user, null: false, foreign_key: true
      t.references :organization, null: false, foreign_key: true
      t.string :role, null: false
      t.datetime :revoked_at
      t.timestamps
    end
    add_index :organization_memberships, [ :application_user_id, :organization_id ],
      unique: true, name: "index_memberships_on_user_and_organization"
    add_check_constraint :organization_memberships,
      "role IN ('owner', 'admin', 'member', 'viewer')", name: "membership_role_valid"
  end
end
