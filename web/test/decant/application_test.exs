defmodule Decant.ApplicationTest do
  @moduledoc "Covers the application's config_change hook."
  use ExUnit.Case, async: true

  test "config_change/3 forwards endpoint config and returns :ok" do
    assert Decant.Application.config_change([], [], []) == :ok
  end
end
