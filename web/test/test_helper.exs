ExUnit.start()
# The archive DB is an external, read-only SQLite fixture (schema owned by the
# Rust `decant` CLI). Tests only read, so we do NOT use the Ecto SQL Sandbox.

# Mimic: make these modules mockable so the daemon-client tests can stub the
# HTTP layer (and `Decant.Daemon.health/0`) without any network access.
Mimic.copy(Req)
Mimic.copy(Decant.Daemon)
