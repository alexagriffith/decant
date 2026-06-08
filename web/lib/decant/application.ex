defmodule Decant.Application do
  # See https://hexdocs.pm/elixir/Application.html
  # for more information on OTP Applications
  @moduledoc false

  use Application

  @impl true
  def start(_type, _args) do
    children =
      [
        DecantWeb.Telemetry,
        Decant.Repo,
        {Ecto.Migrator,
         repos: Application.fetch_env!(:decant, :ecto_repos), skip: skip_migrations?()},
        {DNSCluster, query: Application.get_env(:decant, :dns_cluster_query) || :ignore},
        {Phoenix.PubSub, name: Decant.PubSub},
        # HTTP connection pool for the local decant daemon API.
        {Finch, name: Decant.Finch},
        # Realtime: watch the source dirs and keep the archive fresh.
        Decant.AutoSync
      ] ++
        daemon_client_children() ++
        [
          # Start to serve requests, typically the last entry
          DecantWeb.Endpoint
        ]

    # See https://hexdocs.pm/elixir/Supervisor.html
    # for other strategies and supported options
    opts = [strategy: :one_for_one, name: Decant.Supervisor]
    Supervisor.start_link(children, opts)
  end

  # Tell Phoenix to update the endpoint configuration
  # whenever the application is updated.
  @impl true
  def config_change(changed, _new, removed) do
    DecantWeb.Endpoint.config_change(changed, removed)
    :ok
  end

  defp skip_migrations?() do
    # By default, sqlite migrations are run when using a release
    System.get_env("RELEASE_NAME") == nil
  end

  # The daemon-backed pollers (health + SSE) only run when the client is
  # enabled. Gated off in :test so the suite never opens network connections
  # (mirrors how AutoSync is gated via `config :decant, auto_sync`).
  defp daemon_client_children do
    if Application.get_env(:decant, :daemon_client, false) do
      [Decant.HealthCheck, Decant.DaemonEvents]
    else
      []
    end
  end
end
