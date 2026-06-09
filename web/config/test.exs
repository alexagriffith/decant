import Config

# This app reads everything from the local decant daemon HTTP API; it never
# opens SQLite (no Ecto, no Repo). Don't run the daemon HTTP client pollers in
# tests — they would open real network connections. Tests mock `Decant.Daemon`
# directly via Mimic.
config :decant, daemon_client: false

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

# Emit a JUnit XML report for CI test reporting. The file is written to
# web/report/junit.xml and surfaced in the GitHub UI by the test-reporter step.
config :junit_formatter,
  report_dir: Path.expand("../report", __DIR__),
  report_file: "junit.xml",
  print_report_file: true,
  include_filename?: true,
  include_file_line?: true
