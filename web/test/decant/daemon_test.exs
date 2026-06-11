defmodule Decant.DaemonTest do
  @moduledoc """
  Unit tests for the daemon HTTP client. The HTTP layer (`Req.request/1`) is
  mocked with Mimic so no real network connection is opened: we assert the
  `{data, meta, errors}` envelope is decoded correctly and that each failure mode
  maps to the documented error tuple.

  `Decant.HealthCheck` is exercised against a mocked `Decant.Daemon.health/0`.
  """
  use ExUnit.Case, async: true

  use Mimic

  alias Decant.Daemon

  # A success envelope as the daemon returns it (body is JSON; we set
  # decode_body: false in the client, so the mock returns a raw JSON string).
  defp ok_response(data, meta \\ %{}, version \\ "1") do
    body = Jason.encode!(%{"data" => data, "meta" => meta, "errors" => []})

    {:ok,
     %Req.Response{
       status: 200,
       headers: %{"x-decant-api-version" => [version]},
       body: body
     }}
  end

  describe "envelope decoding" do
    test "list_sessions/1 returns {:ok, data, meta} on a well-formed envelope" do
      data = [%{"id" => 1, "tool" => "claude_code"}]
      meta = %{"pagination" => %{"has_more" => false, "page_size" => 50}}

      Req
      |> expect(:request, fn _req -> ok_response(data, meta) end)

      assert {:ok, ^data, ^meta} = Daemon.list_sessions(limit: 50)
    end

    test "get_session/1 unwraps to {:ok, data} (no meta)" do
      data = %{"summary" => %{"id" => 7}, "stats" => %{}, "messages" => []}

      Req
      |> expect(:request, fn _req -> ok_response(data) end)

      assert {:ok, ^data} = Daemon.get_session(7)
    end

    test "analytics_summary/1 unwraps the object payload to {:ok, data}" do
      data = %{"sessions" => 3, "estimated_cost_usd" => 1.23}

      Req
      |> expect(:request, fn _req -> ok_response(data) end)

      assert {:ok, ^data} = Daemon.analytics_summary([])
    end

    test "health/0 unwraps to {:ok, data}" do
      data = %{"status" => "ok", "api_version" => 1, "db_schema_version" => 1}

      Req
      |> expect(:request, fn _req -> ok_response(data) end)

      assert {:ok, ^data} = Daemon.health()
    end
  end

  describe "request construction" do
    test "sends the Authorization and X-Requested-API-Version headers" do
      System.put_env("DECANT_DAEMON_TOKEN", "deadbeef")
      on_exit(fn -> System.delete_env("DECANT_DAEMON_TOKEN") end)

      Req
      |> expect(:request, fn req ->
        headers = req.headers

        assert ["Bearer deadbeef"] = Map.get(headers, "authorization")
        assert ["1"] = Map.get(headers, "x-requested-api-version")

        ok_response(%{"ok" => true})
      end)

      assert {:ok, _data} = Daemon.health()
    end

    test "search/2 sends a JSON body with the query" do
      Req
      |> expect(:request, fn req ->
        assert req.body == nil
        assert %{q: "auth AND test"} = req.options[:json]
        ok_response([])
      end)

      assert {:ok, [], _meta} = Daemon.search("auth AND test")
    end
  end

  describe "error mapping" do
    test "a refused connection maps to {:error, :service_unavailable}" do
      Req
      |> expect(:request, fn _req ->
        {:error, %Req.TransportError{reason: :econnrefused}}
      end)

      assert {:error, :service_unavailable} = Daemon.list_sessions()
    end

    test "a timeout maps to {:error, :service_unavailable}" do
      Req
      |> expect(:request, fn _req ->
        {:error, %Req.TransportError{reason: :timeout}}
      end)

      assert {:error, :service_unavailable} = Daemon.health()
    end

    test "a 500 maps to {:error, {:http, 500, body}}" do
      body = Jason.encode!(%{"error" => %{"code" => "INTERNAL", "message" => "boom"}})

      Req
      |> expect(:request, fn _req ->
        {:ok, %Req.Response{status: 500, headers: %{"x-decant-api-version" => ["1"]}, body: body}}
      end)

      assert {:error, {:http, 500, decoded}} = Daemon.list_sessions()
      assert decoded["error"]["code"] == "INTERNAL"
    end

    test "a 4xx maps to {:error, {:http, status, _}}" do
      Req
      |> expect(:request, fn _req ->
        {:ok, %Req.Response{status: 401, headers: %{"x-decant-api-version" => ["1"]}, body: ""}}
      end)

      assert {:error, {:http, 401, _}} = Daemon.list_sessions()
    end

    test "a wrong X-Decant-API-Version maps to {:error, {:api_version_mismatch, v}}" do
      Req
      |> expect(:request, fn _req ->
        {:ok, %Req.Response{status: 200, headers: %{"x-decant-api-version" => ["2"]}, body: "{}"}}
      end)

      assert {:error, {:api_version_mismatch, "2"}} = Daemon.list_sessions()
    end
  end

  describe "list and object endpoints" do
    setup do
      # The remaining endpoints all funnel through the same request/handle path;
      # one generic stub lets us assert each one decodes the envelope it expects.
      :ok
    end

    test "by_dimension/2 returns {:ok, rows, meta}" do
      rows = [%{"key" => "codex", "sessions" => 2}]
      Req |> expect(:request, fn _req -> ok_response(rows, %{"m" => 1}) end)
      assert {:ok, ^rows, %{"m" => 1}} = Daemon.by_dimension(:tool, from: "2026-05-01")
    end

    test "activity/1 unwraps to {:ok, data}" do
      data = %{"by_hour" => [], "by_weekday" => []}
      Req |> expect(:request, fn _req -> ok_response(data) end)
      assert {:ok, ^data} = Daemon.activity([])
    end

    test "model_sparklines/1 unwraps to {:ok, data}" do
      data = %{"models" => %{}, "days" => []}
      Req |> expect(:request, fn _req -> ok_response(data) end)
      assert {:ok, ^data} = Daemon.model_sparklines([])
    end

    test "file_hotspots/1 returns {:ok, rows, meta}" do
      Req |> expect(:request, fn _req -> ok_response([], %{}) end)
      assert {:ok, [], _meta} = Daemon.file_hotspots(group: "ext")
    end

    test "tools_usage/1 returns {:ok, rows, meta}" do
      Req |> expect(:request, fn _req -> ok_response([], %{}) end)
      assert {:ok, [], _meta} = Daemon.tools_usage([])
    end

    test "mcp_usage/1 returns {:ok, rows, meta}" do
      Req |> expect(:request, fn _req -> ok_response([], %{}) end)
      assert {:ok, [], _meta} = Daemon.mcp_usage([])
    end

    test "date_bounds/0 unwraps to {:ok, data}" do
      data = %{"min" => "2026-05-01", "max" => "2026-05-02"}
      Req |> expect(:request, fn _req -> ok_response(data) end)
      assert {:ok, ^data} = Daemon.date_bounds()
    end

    test "recommendations/1 unwraps the list to {:ok, list}" do
      recs = [%{"key" => "catalog:agents-md"}]
      Req |> expect(:request, fn _req -> ok_response(recs) end)
      assert {:ok, ^recs} = Daemon.recommendations("all")
    end

    test "recommendations/1 passes through an error tuple" do
      Req
      |> expect(:request, fn _req ->
        {:ok, %Req.Response{status: 500, headers: %{"x-decant-api-version" => ["1"]}, body: ""}}
      end)

      assert {:error, {:http, 500, _}} = Daemon.recommendations("open")
    end

    test "get_session/1 passes through an error tuple" do
      Req
      |> expect(:request, fn _req ->
        {:ok, %Req.Response{status: 404, headers: %{"x-decant-api-version" => ["1"]}, body: "{}"}}
      end)

      assert {:error, {:http, 404, _}} = Daemon.get_session(99)
    end
  end

  describe "error mapping (transport variants)" do
    test "a Mint transport error maps to {:error, :service_unavailable}" do
      Req
      |> expect(:request, fn _req -> {:error, %Mint.TransportError{reason: :closed}} end)

      assert {:error, :service_unavailable} = Daemon.health()
    end

    test "any other exception maps to {:error, {:transport, _}}" do
      Req |> expect(:request, fn _req -> {:error, %RuntimeError{message: "boom"}} end)
      assert {:error, {:transport, %RuntimeError{}}} = Daemon.health()
    end
  end

  describe "envelope and header handling" do
    test "a missing version header is tolerated (decodes the envelope)" do
      body = Jason.encode!(%{"data" => %{"ok" => 1}, "meta" => %{}})

      Req
      |> expect(:request, fn _req ->
        {:ok, %Req.Response{status: 200, headers: %{}, body: body}}
      end)

      assert {:ok, %{"ok" => 1}} = Daemon.health()
    end

    test "list-shaped response headers are read correctly" do
      body = Jason.encode!(%{"data" => %{"ok" => 1}, "meta" => %{}})

      Req
      |> expect(:request, fn _req ->
        {:ok, %Req.Response{status: 200, headers: [{"X-Decant-Api-Version", "1"}], body: body}}
      end)

      assert {:ok, %{"ok" => 1}} = Daemon.health()
    end

    test "a binary-valued version header is read correctly" do
      body = Jason.encode!(%{"data" => %{}, "meta" => %{}})

      Req
      |> expect(:request, fn _req ->
        {:ok, %Req.Response{status: 200, headers: %{"x-decant-api-version" => "1"}, body: body}}
      end)

      assert {:ok, %{}} = Daemon.health()
    end

    test "a 2xx body that is not a valid envelope maps to {:error, {:invalid_envelope, _}}" do
      Req
      |> expect(:request, fn _req ->
        {:ok,
         %Req.Response{
           status: 200,
           headers: %{"x-decant-api-version" => ["1"]},
           body: Jason.encode!(%{"nope" => true})
         }}
      end)

      assert {:error, {:invalid_envelope, %{"nope" => true}}} = Daemon.health()
    end

    test "a non-binary error body is passed through unchanged" do
      Req
      |> expect(:request, fn _req ->
        {:ok,
         %Req.Response{status: 500, headers: %{"x-decant-api-version" => ["1"]}, body: %{a: 1}}}
      end)

      assert {:error, {:http, 500, %{a: 1}}} = Daemon.list_sessions()
    end

    test "a list of headers with a stray non-tuple entry is tolerated" do
      body = Jason.encode!(%{"data" => %{"ok" => 1}, "meta" => %{}})

      Req
      |> expect(:request, fn _req ->
        {:ok,
         %Req.Response{
           status: 200,
           headers: [:weird, {"content-type", "application/json"}],
           body: body
         }}
      end)

      # No version header present in the list → tolerated → envelope decodes.
      assert {:ok, %{"ok" => 1}} = Daemon.health()
    end

    test "a non-JSON error body is surfaced verbatim" do
      Req
      |> expect(:request, fn _req ->
        {:ok,
         %Req.Response{status: 503, headers: %{"x-decant-api-version" => ["1"]}, body: "down"}}
      end)

      assert {:error, {:http, 503, "down"}} = Daemon.list_sessions()
    end
  end

  describe "config helpers" do
    test "base_url/0 honors DECANT_DAEMON_URL and falls back to the default" do
      System.delete_env("DECANT_DAEMON_URL")
      assert Daemon.base_url() == "http://127.0.0.1:4577"

      System.put_env("DECANT_DAEMON_URL", "http://localhost:9999")
      on_exit(fn -> System.delete_env("DECANT_DAEMON_URL") end)
      assert Daemon.base_url() == "http://localhost:9999"
    end

    test "token/0 reads DECANT_DAEMON_TOKEN when set" do
      System.put_env("DECANT_DAEMON_TOKEN", "  abc123  ")
      on_exit(fn -> System.delete_env("DECANT_DAEMON_TOKEN") end)
      assert Daemon.token() == "abc123"
    end

    test "token/0 reads the token file when the env var is blank, else nil" do
      System.put_env("DECANT_DAEMON_TOKEN", "")
      on_exit(fn -> System.delete_env("DECANT_DAEMON_TOKEN") end)

      # Resolve `~` to a per-test sandbox so we never touch the real
      # ~/.decant/daemon.token (Path.expand("~/…") calls System.user_home!/0).
      home = Path.join(System.tmp_dir!(), "decant-token-#{System.unique_integer([:positive])}")
      Mimic.stub(System, :user_home!, fn -> home end)
      path = Path.join([home, ".decant", "daemon.token"])
      File.mkdir_p!(Path.dirname(path))
      on_exit(fn -> File.rm_rf!(home) end)

      File.write!(path, "  filetoken  \n")
      assert Daemon.token() == "filetoken"

      File.rm_rf!(path)
      assert Daemon.token() == nil
    end
  end
end
