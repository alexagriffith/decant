defmodule Decant.Application do
  @moduledoc false

  use Application

  @impl true
  def start(_type, _args) do
    children =
      [
        DecantWeb.Telemetry,
        {DNSCluster, query: Application.get_env(:decant, :dns_cluster_query) || :ignore},
        {Phoenix.PubSub, name: Decant.PubSub},
        {Finch, name: Decant.Finch}
      ] ++
        daemon_client_children() ++
        [
          DecantWeb.Endpoint
        ]

    opts = [strategy: :one_for_one, name: Decant.Supervisor]
    Supervisor.start_link(children, opts)
  end

  @impl true
  def config_change(changed, _new, removed) do
    DecantWeb.Endpoint.config_change(changed, removed)
    :ok
  end

  # The daemon-backed pollers (health + SSE) only run when the client is
  # enabled. Gated off in :test (via `config :decant, :daemon_client`) so the
  # suite never opens network connections.
  defp daemon_client_children do
    if Application.get_env(:decant, :daemon_client, false) do
      [Decant.HealthCheck, Decant.DaemonEvents]
    else
      []
    end
  end
end
