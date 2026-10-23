class ApplicationController < ActionController::API
  before_action do
    response.headers["Cache-Control"] = "no-store"
    response.headers["X-Content-Type-Options"] = "nosniff"
  end
end
