defmodule DecantWeb.FilesLiveTest do
  use DecantWeb.ConnCase, async: false
  use Mimic

  import Phoenix.LiveViewTest

  setup :set_mimic_global

  setup do
    Decant.DaemonStubs.install()
    :ok
  end

  describe "Archive.file_hotspots/2" do
    test "maps rows, adds total, defaults on missing counts" do
      rows = Decant.Archive.file_hotspots(%{}, group: :path)
      assert [first | _] = rows
      assert first.key == "src/main.rs"
      assert first.project == "/Users/dev/proj"
      assert first.total == 9
      assert Enum.all?(rows, &is_integer(&1.total))
    end

    test "threads group/op params through to the daemon" do
      Mimic.expect(Decant.Daemon, :file_hotspots, fn opts ->
        assert opts[:group] == "ext"
        assert opts[:op] == "edit"
        {:ok, [], %{}}
      end)

      assert Decant.Archive.file_hotspots(%{}, group: :ext, op: :edit) == []
    end

    test "returns [] on daemon error" do
      Mimic.stub(Decant.Daemon, :file_hotspots, fn _ -> {:error, :down} end)
      assert Decant.Archive.file_hotspots(%{}) == []
    end
  end

  describe "FilesLive" do
    test "renders hotspot table with totals and project basenames", %{conn: conn} do
      {:ok, _view, html} = live(conn, ~p"/files")
      assert html =~ "File hotspots"
      assert html =~ "src/main.rs"
      assert html =~ "AGENTS.md"
      # total = 5+3+1+0 for main.rs, ranked first by default total sort
      assert html =~ ">9<"
      # project renders as its basename, full path kept in the title attr
      assert html =~ ~r/>\s*proj\s*</
      assert html =~ ~s(title="/Users/dev/proj")
    end

    test "group=ext switches the key column and drops project", %{conn: conn} do
      {:ok, _view, html} = live(conn, ~p"/files?group=ext")
      assert html =~ ~r/>\s*rs\s*</
      refute html =~ "src/main.rs"
      refute html =~ ">Project<"
    end

    test "op param threads to the client and empty result shows empty state", %{conn: conn} do
      test_pid = self()

      Mimic.stub(Decant.Daemon, :file_hotspots, fn opts ->
        send(test_pid, {:file_opts, opts})
        {:ok, [], %{}}
      end)

      {:ok, _view, html} = live(conn, ~p"/files?op=edit")
      assert html =~ "No file activity"
      assert_receive {:file_opts, opts}
      assert opts[:op] == "edit"
    end

    test "sort event reorders rows", %{conn: conn} do
      {:ok, view, html} = live(conn, ~p"/files")
      # default total sort: main.rs (9) before AGENTS.md (8)
      assert html =~ ~r/src\/main\.rs.*AGENTS\.md/s
      html = render_click(view, "sort", %{"table" => "files", "col" => "reads"})
      # reads desc puts AGENTS.md (8 reads) before main.rs (5 reads)
      assert html =~ ~r/AGENTS\.md.*src\/main\.rs/s
    end

    test "unknown group/op fall back to defaults", %{conn: conn} do
      {:ok, _view, html} = live(conn, ~p"/files?group=bogus&op=bogus")
      assert html =~ "src/main.rs"
    end

    test "a path-mode row with no project renders the middot placeholder", %{conn: conn} do
      Mimic.stub(Decant.Daemon, :file_hotspots, fn _opts ->
        {:ok,
         [
           %{
             "key" => "orphan.txt",
             "project" => nil,
             "reads" => 1,
             "edits" => 0,
             "writes" => 0,
             "deletes" => 0,
             "sessions" => 1,
             "last_touched_at" => "2026-05-01T09:00:00Z"
           }
         ], %{}}
      end)

      {:ok, _view, html} = live(conn, ~p"/files")
      assert html =~ "orphan.txt"
    end

    test "key links to a quoted FTS search", %{conn: conn} do
      {:ok, _view, html} = live(conn, ~p"/files")
      assert html =~ ~s(/search?q=)
    end

    test "date presets and filter chips preserve the group/op view", %{conn: conn} do
      {:ok, _view, html} = live(conn, ~p"/files?group=ext&op=edit&tool=claude_code")
      # Every shared-control link (presets, chip removal) must carry the view
      # params, or switching ranges silently resets the view.
      preset_links = Regex.scan(~r/href="\/files\?[^"]*"/, html) |> List.flatten()
      assert preset_links != []

      with_dates = Enum.filter(preset_links, &(&1 =~ "from="))
      assert with_dates != [], "expected date-preset links"

      assert Enum.all?(with_dates, &(&1 =~ "group=ext")),
             "date links must keep group=ext: #{inspect(Enum.take(with_dates, 2))}"

      assert Enum.all?(with_dates, &(&1 =~ "op=edit")),
             "date links must keep op=edit"
    end
  end

  describe "SearchLive deep link" do
    test "q in the URL pre-fills and runs the search", %{conn: conn} do
      {:ok, _view, html} = live(conn, ~p"/search?q=auth")
      assert html =~ "auth"
      refute html =~ "Search your archive"
    end

    test "navigating back to /search without q resets the stale results", %{conn: conn} do
      {:ok, view, html} = live(conn, ~p"/search?q=auth")
      refute html =~ "Search your archive"

      html = render_patch(view, ~p"/search")
      assert html =~ "Search your archive", "results must clear when q leaves the URL"
    end

    test "whitespace-only q is treated as empty", %{conn: conn} do
      {:ok, _view, html} = live(conn, ~p"/search?q=%20%20")
      assert html =~ "Search your archive"
    end
  end
end
