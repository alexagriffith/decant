defmodule Decant.Daemon do
  @moduledoc """
  Thin HTTP+JSON client for the local decant daemon (the Rust process that owns
  the archive and exposes a read API on `http://127.0.0.1:4577`).

  All `/api/*` requests carry an `Authorization: Bearer <token>` header and the
  `X-Requested-API-Version: 1` header, and target loopback with a `127.0.0.1`
  `Host`. Requests go through a named `Finch` pool (`Decant.Finch`) started in the
  supervision tree.

  Every endpoint returns the daemon's `{data, meta, errors}` envelope. The
  private `request/4` helper decodes that envelope and normalizes outcomes to:

    * `{:ok, data, meta}` — 2xx with a well-formed envelope
    * `{:error, :service_unavailable}` — connection refused / timed out
    * `{:error, {:http, status, body}}` — any 4xx/5xx response
    * `{:error, {:api_version_mismatch, version}}` — the response's
      `X-Decant-API-Version` header is not `"1"`

  Public functions mirror the API and return `{:ok, ...}` / `{:error, ...}`.
  Lists return `{:ok, data, meta}` so callers can read pagination from `meta`;
  scalar/object endpoints return `{:ok, data}`.

  This module is a pure client: it never writes to the DB and holds no state.
  """

  @api_version "1"
  @default_base_url "http://127.0.0.1:4577"
  @default_token_path "~/.decant/daemon.token"
  @receive_timeout 15_000

  # --- configuration helpers -------------------------------------------------

  @doc """
  Base URL of the daemon. `DECANT_DAEMON_URL` env overrides the default
  (`#{@default_base_url}`).
  """
  def base_url do
    System.get_env("DECANT_DAEMON_URL") || @default_base_url
  end

  @doc """
  Bearer token for the daemon. `DECANT_DAEMON_TOKEN` env overrides the file at
  `#{@default_token_path}`. Returns `nil` if neither is available (requests then
  fail with the daemon's `401`, surfaced as `{:error, {:http, 401, _}}`).
  """
  def token do
    case System.get_env("DECANT_DAEMON_TOKEN") do
      nil -> read_token_file()
      "" -> read_token_file()
      value -> String.trim(value)
    end
  end

  defp read_token_file do
    path = Path.expand(@default_token_path)

    case File.read(path) do
      {:ok, contents} -> String.trim(contents)
      {:error, _} -> nil
    end
  end

  # --- public API ------------------------------------------------------------

  @doc "List session summaries. `opts` becomes query params (`from`, `to`, `tool`, `model`, `project`, `sort`, `limit`, `cursor`)."
  def list_sessions(opts \\ []) do
    request(:get, "/sessions", params: opts)
  end

  @doc "Fetch one session's detail (summary + stats + messages) by id."
  def get_session(id) do
    case request(:get, "/sessions/#{id}") do
      {:ok, data, _meta} -> {:ok, data}
      error -> error
    end
  end

  @doc "Full-text search over message blocks. `q` is the FTS5 query; `opts` may set `limit`/`cursor`."
  def search(q, opts \\ []) do
    body = Enum.into(opts, %{q: q})
    request(:post, "/search", json: body)
  end

  @doc "Aggregate totals scoped to the given filter `opts`."
  def analytics_summary(opts \\ []) do
    single(:get, "/analytics/summary", params: opts)
  end

  @doc "Ranked rollups for `dim` (`:tool`, `:model`, `:project`, `:day`). `opts` are filters + pagination."
  def by_dimension(dim, opts \\ []) do
    request(:get, "/analytics/by-dimension", params: [{:dim, dim} | opts])
  end

  @doc "Activity histograms (by local hour and weekday) for the given filter `opts`."
  def activity(opts \\ []) do
    single(:get, "/analytics/activity", params: opts)
  end

  @doc "Per-model daily session counts on a shared day axis."
  def model_sparklines(opts \\ []) do
    single(:get, "/analytics/model-sparklines", params: opts)
  end

  @doc "Per-tool usage rows (calls/errors/error rate)."
  def tools_usage(opts \\ []) do
    request(:get, "/tools/usage", params: opts)
  end

  @doc "Per-MCP-server usage rows."
  def mcp_usage(opts \\ []) do
    request(:get, "/tools/mcp-usage", params: opts)
  end

  @doc "Min/max session date in the archive."
  def date_bounds do
    single(:get, "/metadata/date-bounds")
  end

  @doc """
  List recommendations (data-derived signals + the evergreen catalog) with
  lifecycle state. `status` is one of `"open"`, `"implemented"`, or `"all"`.
  Returns `{:ok, list}` of string-keyed rec maps, or an `{:error, ...}` tuple.
  """
  def recommendations(status \\ "open") do
    case request(:get, "/recommendations", params: [status: status]) do
      {:ok, data, _meta} -> {:ok, data}
      error -> error
    end
  end

  @doc """
  Unauthenticated liveness probe. Returns `{:ok, data}` with `api_version` /
  `db_schema_version` / `status`, or an `{:error, ...}` tuple. Note the daemon's
  `/health` does not require a token, but we still send the headers; they are
  harmless.
  """
  def health do
    single(:get, "/health")
  end

  # --- internals -------------------------------------------------------------

  # Endpoints whose `data` is a scalar/object (not a paginated list): drop `meta`.
  defp single(method, path, opts \\ []) do
    case request(method, path, opts) do
      {:ok, data, _meta} -> {:ok, data}
      error -> error
    end
  end

  # One request helper for the whole client. Builds the request against the
  # `Decant.Finch` pool with auth + version headers, then normalizes the result.
  defp request(method, path, opts \\ []) do
    params = opts |> Keyword.get(:params, []) |> drop_nil_params()

    req =
      [
        method: method,
        base_url: base_url(),
        url: "/api/v1" <> path,
        finch: Decant.Finch,
        headers: headers(),
        receive_timeout: @receive_timeout,
        retry: false,
        # Treat all statuses as data; we branch on them ourselves below.
        decode_body: false
      ]
      |> maybe_put(:params, params)
      |> maybe_put(:json, Keyword.get(opts, :json))
      |> Req.new()

    case Req.request(req) do
      {:ok, resp} -> handle_response(resp)
      {:error, exception} -> handle_error(exception)
    end
  end

  defp headers do
    base = [{"x-requested-api-version", @api_version}]

    case token() do
      nil -> base
      tok -> [{"authorization", "Bearer " <> tok} | base]
    end
  end

  defp handle_response(%Req.Response{status: status, headers: headers, body: body}) do
    cond do
      not version_ok?(headers) ->
        {:error, {:api_version_mismatch, response_version(headers)}}

      status in 200..299 ->
        decode_envelope(body)

      true ->
        {:error, {:http, status, decode_maybe_json(body)}}
    end
  end

  # Connection refused / closed / timeout → the daemon is down or unreachable.
  defp handle_error(%Req.TransportError{}), do: {:error, :service_unavailable}
  defp handle_error(%Mint.TransportError{}), do: {:error, :service_unavailable}
  defp handle_error(%{__exception__: true} = other), do: {:error, {:transport, other}}

  # The daemon stamps every response (incl. /health) with X-Decant-API-Version.
  # A missing header is tolerated (older/edge cases); a present-but-wrong one is
  # a hard mismatch.
  defp version_ok?(headers) do
    case response_version(headers) do
      nil -> true
      v -> v == @api_version
    end
  end

  defp response_version(headers) do
    headers
    |> get_header("x-decant-api-version")
    |> case do
      nil -> nil
      value -> to_string(value)
    end
  end

  # Req normalizes header names to lowercase; values are lists.
  defp get_header(headers, name) when is_map(headers) do
    case Map.get(headers, name) do
      [value | _] -> value
      value when is_binary(value) -> value
      _ -> nil
    end
  end

  defp get_header(headers, name) when is_list(headers) do
    Enum.find_value(headers, fn
      {k, v} -> if String.downcase(to_string(k)) == name, do: v
      _ -> nil
    end)
  end

  defp decode_envelope(body) do
    case decode_maybe_json(body) do
      %{"data" => data} = envelope -> {:ok, data, Map.get(envelope, "meta", %{})}
      other -> {:error, {:invalid_envelope, other}}
    end
  end

  defp decode_maybe_json(body) when is_binary(body) do
    case Jason.decode(body) do
      {:ok, decoded} -> decoded
      {:error, _} -> body
    end
  end

  defp decode_maybe_json(body), do: body

  defp drop_nil_params(params) do
    Enum.reject(params, fn {_k, v} -> is_nil(v) end)
  end

  defp maybe_put(opts, _key, nil), do: opts
  defp maybe_put(opts, _key, []), do: opts
  defp maybe_put(opts, key, value), do: Keyword.put(opts, key, value)
end
