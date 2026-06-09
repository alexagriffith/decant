ExUnit.start()
# This app reads all its data from the local decant daemon HTTP API; it never
# opens SQLite (no Ecto, no Repo), so there is no database sandbox to set up.

# Mimic: make these modules mockable so the daemon-client tests can stub the
# HTTP layer (and `Decant.Daemon.health/0`) without any network access.
Mimic.copy(Req)
Mimic.copy(Decant.Daemon)
Mimic.copy(Decant.AgentLauncher)
