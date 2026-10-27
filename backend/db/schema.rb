# This file is auto-generated from the current state of the database. Instead
# of editing this file, please use the migrations feature of Active Record to
# incrementally modify your database, and then regenerate this schema definition.
#
# This file is the source Rails uses to define your schema when running `bin/rails
# db:schema:load`. When creating a new database, `bin/rails db:schema:load` tends to
# be faster and is potentially less error prone than running all of your
# migrations from scratch. Old migrations may fail to apply correctly if those
# migrations use external dependencies or application code.
#
# It's strongly recommended that you check this file into your version control system.

ActiveRecord::Schema[8.1].define(version: 2026_10_27_010000) do
  create_table "application_users", force: :cascade do |t|
    t.string "public_id", null: false
    t.string "display_name", null: false
    t.boolean "active", default: true, null: false
    t.datetime "created_at", null: false
    t.datetime "updated_at", null: false
    t.index ["public_id"], name: "index_application_users_on_public_id", unique: true
  end

  create_table "external_identities", force: :cascade do |t|
    t.integer "application_user_id", null: false
    t.string "issuer", null: false
    t.string "subject", null: false
    t.datetime "revoked_at"
    t.datetime "created_at", null: false
    t.datetime "updated_at", null: false
    t.index ["application_user_id"], name: "index_external_identities_on_application_user_id"
    t.index ["issuer", "subject"], name: "index_external_identities_on_issuer_and_subject", unique: true
  end

  create_table "organization_memberships", force: :cascade do |t|
    t.integer "application_user_id", null: false
    t.integer "organization_id", null: false
    t.string "role", null: false
    t.datetime "revoked_at"
    t.datetime "created_at", null: false
    t.datetime "updated_at", null: false
    t.index ["application_user_id", "organization_id"], name: "index_memberships_on_user_and_organization", unique: true
    t.index ["application_user_id"], name: "index_organization_memberships_on_application_user_id"
    t.index ["organization_id"], name: "index_organization_memberships_on_organization_id"
    t.check_constraint "role IN ('owner', 'admin', 'member', 'viewer')", name: "membership_role_valid"
  end

  create_table "organizations", force: :cascade do |t|
    t.string "public_id", null: false
    t.string "name", null: false
    t.boolean "active", default: true, null: false
    t.datetime "created_at", null: false
    t.datetime "updated_at", null: false
    t.index ["public_id"], name: "index_organizations_on_public_id", unique: true
  end

  add_foreign_key "external_identities", "application_users"
  add_foreign_key "organization_memberships", "application_users"
  add_foreign_key "organization_memberships", "organizations"
end
