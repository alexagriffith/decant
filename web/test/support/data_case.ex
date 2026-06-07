defmodule Decant.DataCase do
  @moduledoc """
  This module defines the setup for tests requiring
  access to the application's data layer.

  You may define functions here to be used as helpers in
  your tests.

  The archive DB is an external, read-only SQLite fixture (its schema is owned
  by the Rust `decant` CLI). This app only reads it, so there is no Ecto SQL
  Sandbox to set up or tear down. Because every test only reads the fixture,
  cases may safely run with `async: true`.
  """

  use ExUnit.CaseTemplate

  using do
    quote do
      alias Decant.Repo

      import Ecto
      import Ecto.Changeset
      import Ecto.Query
      import Decant.DataCase
    end
  end

  setup _tags do
    :ok
  end

  @doc """
  A helper that transforms changeset errors into a map of messages.

      assert {:error, changeset} = Accounts.create_user(%{password: "short"})
      assert "password is too short" in errors_on(changeset).password
      assert %{password: ["password is too short"]} = errors_on(changeset)

  """
  def errors_on(changeset) do
    Ecto.Changeset.traverse_errors(changeset, fn {message, opts} ->
      Regex.replace(~r"%{(\w+)}", message, fn _, key ->
        opts |> Keyword.get(String.to_existing_atom(key), key) |> to_string()
      end)
    end)
  end
end
