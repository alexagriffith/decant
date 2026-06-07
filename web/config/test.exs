import Config

# Configure the database.
#
# The archive schema is owned and written by the external Rust `decant` CLI;
# this app only reads it. Tests run against a committed read-only fixture DB,
# so we use a normal pool (NOT Ecto.Adapters.SQL.Sandbox) — there are no
# Ecto-managed migrations and no writes to roll back.
config :decant, Decant.Repo,
  database: Path.expand("../test/fixtures/decant.db", __DIR__),
  pool_size: 5

# We don't run a server during test. If one is required,
# you can enable the server option below.
config :decant, DecantWeb.Endpoint,
  http: [ip: {127, 0, 0, 1}, port: 4002],
  secret_key_base: "T3kE5P8feQsBAyoXIMy4VASsIfT1a19/grZk5UUxEdCiOn7YxZyOyP88L9z8jwKd",
  server: false

# Print only warnings and errors during test
config :logger, level: :warning

# Initialize plugs at runtime for faster test compilation
config :phoenix, :plug_init_mode, :runtime

# Enable helpful, but potentially expensive runtime checks
config :phoenix_live_view,
  enable_expensive_runtime_checks: true

# Sort query params output of verified routes for robust url comparisons
config :phoenix,
  sort_verified_routes_query_params: true
