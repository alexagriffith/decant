import Testing

@testable import DecantKit

@Suite("SSELineParser")
struct SSETests {
    @Test func namedEventDispatchesOnBlankLine() {
        var p = SSELineParser()
        #expect(p.feed("event: session_activity") == nil)
        #expect(p.feed(#"data: {"state":"active"}"#) == nil)
        let event = p.feed("")
        #expect(event == ServerEvent(name: "session_activity", data: #"{"state":"active"}"#))
        // Frame state resets after dispatch.
        #expect(p.feed("") == nil)
    }

    @Test func multipleDataLinesJoinWithNewline() {
        var p = SSELineParser()
        _ = p.feed("data: one")
        _ = p.feed("data: two")
        #expect(p.feed("") == ServerEvent(name: "message", data: "one\ntwo"))
    }

    @Test func keepAlivesAndUnknownFieldsAreIgnored() {
        var p = SSELineParser()
        #expect(p.feed(": keep-alive") == nil)
        #expect(p.feed("") == nil, "a comment alone is not a frame")
        _ = p.feed("id: 42")
        _ = p.feed("retry: 1000")
        #expect(p.feed("") == nil, "id/retry alone do not make a frame")
        _ = p.feed("event: resync")
        _ = p.feed(#"data: {"type":"resync"}"#)
        let event = p.feed("")
        #expect(event?.name == "resync")
    }

    @Test func dataWithoutLeadingSpaceParses() {
        var p = SSELineParser()
        _ = p.feed("data:x")
        #expect(p.feed("")?.data == "x")
    }
}

@Suite("SSEByteSplitter")
struct SplitterTests {
    private func lines(_ input: String) -> [String] {
        var s = SSEByteSplitter()
        var out: [String] = []
        for b in Array(input.utf8) {
            if let line = s.feed(b) { out.append(line) }
        }
        return out
    }

    @Test func preservesEmptyLinesUnlikeAsyncLines() {
        #expect(lines("a\n\nb\n") == ["a", "", "b"])
    }

    @Test func foldsCRLF() {
        #expect(lines("event: x\r\n\r\n") == ["event: x", ""])
    }

    @Test func buffersPartialChunks() {
        var s = SSEByteSplitter()
        var out: [String] = []
        for chunk in ["eve", "nt: x", "\n", "\n"] {
            for b in Array(chunk.utf8) {
                if let line = s.feed(b) { out.append(line) }
            }
        }
        #expect(out == ["event: x", ""])
    }

    @Test func splitterPlusParserEndToEnd() {
        var s = SSEByteSplitter()
        var p = SSELineParser()
        var events: [ServerEvent] = []
        let wire = ": hi\n\nevent: archive_updated\ndata: {\"ingested\":3}\n\n"
        for b in Array(wire.utf8) {
            if let line = s.feed(b), let e = p.feed(line) {
                events.append(e)
            }
        }
        #expect(events == [ServerEvent(name: "archive_updated", data: #"{"ingested":3}"#)])
    }
}
