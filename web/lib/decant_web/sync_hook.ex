defmodule DecantWeb.SyncHook do
  @moduledoc """
  Global LiveView `on_mount` hook for realtime archive updates.

  On connect it subscribes each LiveView to the `"archive"` PubSub topic — fed by
  `Decant.DaemonEvents` from the daemon's change-stream — and live-reloads the
  current page (`push_patch`, preserving filters) whenever the daemon ingests new
  sessions, so the dashboard updates in realtime. It also tracks daemon readiness
  (`Decant.HealthCheck.ready?/0`) for the service-down banner. There is no manual
  sync control: the daemon keeps the archive current. No per-LiveView code is
  required.
  """
  import Phoenix.LiveView
  import Phoenix.Component, only: [assign: 3, assign_new: 3]

  @topic "archive"

  def on_mount(:default, _params, _session, socket) do
    if connected?(socket), do: Phoenix.PubSub.subscribe(Decant.PubSub, @topic)

    socket =
      socket
      |> assign_new(:current_url, fn -> "/" end)
      |> assign_new(:daemon_ready, &Decant.HealthCheck.ready?/0)
      |> assign_new(:archive_meta, &Decant.Archive.overview/0)
      |> attach_hook(:decant_sync_params, :handle_params, &store_url/3)
      |> attach_hook(:decant_sync_info, :handle_info, &on_info/2)

    {:cont, socket}
  end

  defp store_url(_params, uri, socket) do
    parsed = URI.parse(uri)
    url = parsed.path <> if(parsed.query, do: "?" <> parsed.query, else: "")
    {:cont, assign(socket, :current_url, url)}
  end

  defp on_info({:archive_updated, _report}, socket) do
    {:halt,
     socket
     |> assign(:daemon_ready, Decant.HealthCheck.ready?())
     |> assign(:archive_meta, Decant.Archive.overview())
     |> push_patch(to: socket.assigns.current_url)}
  end

  defp on_info(_msg, socket), do: {:cont, socket}
end
