defmodule DecantWeb.Router do
  use DecantWeb, :router

  pipeline :browser do
    plug :accepts, ["html"]
    plug :fetch_session
    plug :fetch_live_flash
    plug :put_root_layout, html: {DecantWeb.Layouts, :root}
    plug :protect_from_forgery
    plug :put_secure_browser_headers
  end

  pipeline :api do
    plug :accepts, ["json"]
  end

  scope "/", DecantWeb do
    pipe_through :browser

    live "/", SessionLive.Index, :index
    live "/sessions/:id", SessionLive.Show, :show
    live "/search", SessionLive.Search, :search
    live "/analytics", AnalyticsLive, :index
    live "/tools", ToolsLive, :index
  end

  # Other scopes may use custom stacks.
  # scope "/api", DecantWeb do
  #   pipe_through :api
  # end
end
