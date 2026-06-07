ExUnit.start()
# The archive DB is an external, read-only SQLite fixture (schema owned by the
# Rust `decant` CLI). Tests only read, so we do NOT use the Ecto SQL Sandbox.
