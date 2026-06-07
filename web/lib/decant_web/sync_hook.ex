defmodule DecantWeb.SyncHook do
  @moduledoc """
  Global LiveView `on_mount` hook for realtime archive updates.

  On connect it subscribes each LiveView to `Decant.AutoSync` broadcasts and
  live-reloads the current page (`push_patch`, preserving filters) whenever new
  sessions are ingested — so the dashboard updates in realtime. It also powers
  the manual "Sync" button by routing to `Decant.AutoSync.sync_now/0`. No
  per-LiveView code is required.
  """
  import Phoenix.LiveView
  import Phoenix.Component, only: [assign: 3, assign_new: 3]

  def on_mount(:default, _params, _session, socket) do
    if connected?(socket), do: Phoenix.PubSub.subscribe(Decant.PubSub, Decant.AutoSync.topic())

    socket =
      socket
      |> assign_new(:syncing, fn -> false end)
      |> assign_new(:current_url, fn -> "/" end)
      |> attach_hook(:decant_sync_params, :handle_params, &store_url/3)
      |> attach_hook(:decant_sync_event, :handle_event, &on_event/3)
      |> attach_hook(:decant_sync_info, :handle_info, &on_info/2)

    {:cont, socket}
  end

  defp store_url(_params, uri, socket) do
    parsed = URI.parse(uri)
    url = parsed.path <> if(parsed.query, do: "?" <> parsed.query, else: "")
    {:cont, assign(socket, :current_url, url)}
  end

  defp on_event("sync", _params, socket) do
    Decant.AutoSync.sync_now()
    {:halt, assign(socket, :syncing, true)}
  end

  defp on_event(_event, _params, socket), do: {:cont, socket}

  defp on_info({:archive_updated, report}, socket) do
    socket =
      if socket.assigns[:syncing],
        do: put_flash(socket, :info, "Synced. #{report}"),
        else: socket

    {:halt, socket |> assign(:syncing, false) |> push_patch(to: socket.assigns.current_url)}
  end

  defp on_info(_msg, socket), do: {:cont, socket}
end
