# Emit a JUnit XML report under CI (consumed by the test-reporter step); kept
# off locally to avoid generating files and console noise on every `mix test`.
# Formatters must be configured before ExUnit starts, and the report directory
# must exist first because junit_formatter does not create it.
if System.get_env("CI") do
  File.mkdir_p!(Application.fetch_env!(:junit_formatter, :report_dir))
  ExUnit.configure(formatters: [JUnitFormatter, ExUnit.CLIFormatter])
end

ExUnit.start()
# This app reads all its data from the local decant daemon HTTP API; it never
# opens SQLite (no Ecto, no Repo), so there is no database sandbox to set up.

# Mimic: make these modules mockable so the daemon-client tests can stub the
# HTTP layer (and `Decant.Daemon.health/0`) without any network access.
Mimic.copy(Req)
Mimic.copy(Decant.Daemon)
Mimic.copy(Decant.AgentLauncher)
Mimic.copy(Decant.Archive)
Mimic.copy(Decant.Settings)
Mimic.copy(System)
