defmodule DecantWeb.ConnCase do
  @moduledoc """
  This module defines the test case to be used by
  tests that require setting up a connection.

  Such tests rely on `Phoenix.ConnTest` and also
  import other functionality to make it easier
  to build common data structures and query the data layer.

  The archive DB is an external, read-only SQLite fixture (its schema is owned
  by the Rust `decant` CLI). This app only reads it, so there is no Ecto SQL
  Sandbox to set up or tear down — tests share the same read-only connection
  pool. Because every test only reads the fixture, cases may safely run with
  `async: true`.
  """

  use ExUnit.CaseTemplate

  using do
    quote do
      # The default endpoint for testing
      @endpoint DecantWeb.Endpoint

      use DecantWeb, :verified_routes

      # Import conveniences for testing with connections
      import Plug.Conn
      import Phoenix.ConnTest
      import DecantWeb.ConnCase
    end
  end

  setup _tags do
    {:ok, conn: Phoenix.ConnTest.build_conn()}
  end
end
