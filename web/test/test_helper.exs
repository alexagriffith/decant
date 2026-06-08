# Emit a JUnit XML report under CI (consumed by the test-reporter step); kept
# off locally to avoid generating files and console noise on every `mix test`.
# Formatters must be configured before ExUnit starts, and the report directory
# must exist first because junit_formatter does not create it.
if System.get_env("CI") do
  File.mkdir_p!(Application.fetch_env!(:junit_formatter, :report_dir))
  ExUnit.configure(formatters: [JUnitFormatter, ExUnit.CLIFormatter])
end

ExUnit.start()
# The archive DB is an external, read-only SQLite fixture (schema owned by the
# Rust `decant` CLI). Tests only read, so we do NOT use the Ecto SQL Sandbox.
