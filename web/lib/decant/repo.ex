defmodule Decant.Repo do
  use Ecto.Repo,
    otp_app: :decant,
    adapter: Ecto.Adapters.SQLite3
end
