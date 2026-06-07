defmodule DecantWeb.SyncHook do
  @moduledoc """
  Global LiveView `on_mount` hook powering the "Sync" control in the app shell.

  Handles the `"sync"` event on any page: runs `decant sync` in a background
  Task, flips the `:syncing` assign for the spinner, then flashes the result and
  re-navigates to the current page so its data refreshes. No per-LiveView code is
  required — every page gets a working, consistent sync button.
  """
  import Phoenix.LiveView
  import Phoenix.Component, only: [assign: 3, assign_new: 3]

  alias Decant.Archive

  def on_mount(:default, _params, _session, socket) do
    socket =
      socket
      |> assign_new(:syncing, fn -> false end)
      |> assign_new(:current_path, fn -> "/" end)
      |> attach_hook(:decant_sync_params, :handle_params, &store_path/3)
      |> attach_hook(:decant_sync_event, :handle_event, &on_event/3)
      |> attach_hook(:decant_sync_info, :handle_info, &on_info/2)

    {:cont, socket}
  end

  defp store_path(_params, uri, socket) do
    {:cont, assign(socket, :current_path, URI.parse(uri).path || "/")}
  end

  defp on_event("sync", _params, %{assigns: %{syncing: true}} = socket), do: {:halt, socket}

  defp on_event("sync", _params, socket) do
    parent = self()
    bin = System.get_env("DECANT_BIN") || "decant"
    db = Archive.db_path()
    Task.start(fn -> send(parent, {:decant_sync_done, run(bin, db)}) end)
    {:halt, assign(socket, :syncing, true)}
  end

  defp on_event(_event, _params, socket), do: {:cont, socket}

  defp on_info({:decant_sync_done, msg}, socket) do
    {:halt,
     socket
     |> assign(:syncing, false)
     |> put_flash(:info, "Sync — #{msg}")
     |> push_navigate(to: socket.assigns.current_path)}
  end

  defp on_info(_msg, socket), do: {:cont, socket}

  defp run(_bin, nil), do: "archive DB not configured (set DECANT_DB)"

  defp run(bin, db) do
    case System.cmd(bin, ["--db", db, "sync"], stderr_to_stdout: true) do
      {out, 0} -> String.trim(out)
      {out, 3} -> String.trim(out) <> " (with issues)"
      {out, code} -> "exit #{code}: #{String.slice(out, 0, 200)}"
    end
  rescue
    e -> "error: #{Exception.message(e)} (is `decant` on PATH? set DECANT_BIN)"
  end
end
