import Foundation

/// One Server-Sent Event as the daemon emits them on `/api/v1/events`
/// (`archive_updated`, `session_activity`, `resync`).
public struct ServerEvent: Sendable, Equatable {
    public let name: String
    public let data: String

    public init(name: String, data: String) {
        self.name = name
        self.data = data
    }
}

/// Incremental SSE frame parser. Feed lines (without terminators); a blank
/// line dispatches the buffered frame. Comments (`:` keep-alives) and unknown
/// fields are ignored; malformed input never throws — it just doesn't emit.
public struct SSELineParser: Sendable {
    private var name = ""
    private var dataLines: [String] = []

    public init() {}

    public mutating func feed(_ line: String) -> ServerEvent? {
        if line.isEmpty {
            defer {
                name = ""
                dataLines = []
            }
            guard !dataLines.isEmpty || !name.isEmpty else { return nil }
            return ServerEvent(name: name.isEmpty ? "message" : name, data: dataLines.joined(separator: "\n"))
        }
        if line.hasPrefix(":") { return nil }

        let (field, value): (Substring, Substring)
        if let colon = line.firstIndex(of: ":") {
            field = line[..<colon]
            var v = line[line.index(after: colon)...]
            if v.hasPrefix(" ") { v = v.dropFirst() }
            value = v
        } else {
            field = line[...]
            value = ""
        }

        switch field {
        case "event": name = String(value)
        case "data": dataLines.append(String(value))
        default: break  // id/retry/unknown: not needed by this client
        }
        return nil
    }
}

/// Byte-level line splitter. `URLSession.AsyncBytes.lines` omits empty lines —
/// which are SSE's frame delimiters — so we split ourselves, preserving
/// empties and folding `\r\n` to one terminator.
public struct SSEByteSplitter: Sendable {
    private var buffer: [UInt8] = []

    public init() {}

    public mutating func feed(_ byte: UInt8) -> String? {
        if byte == UInt8(ascii: "\n") {
            if buffer.last == UInt8(ascii: "\r") { buffer.removeLast() }
            defer { buffer = [] }
            return String(decoding: buffer, as: UTF8.self)
        }
        buffer.append(byte)
        return nil
    }
}

/// Streams the daemon's change events. Each connection is one long-lived GET;
/// the stream finishes (or throws) when the connection drops — reconnect policy
/// lives in `NowStore`, not here.
public struct SSEClient: Sendable {
    private let session: URLSession

    public init(session: URLSession = .shared) {
        self.session = session
    }

    public func events(config: DaemonConfig) -> AsyncThrowingStream<ServerEvent, Error> {
        AsyncThrowingStream { continuation in
            let task = Task {
                var request = URLRequest(url: config.baseURL.appendingPathComponent("api/v1/events"))
                request.timeoutInterval = 3600
                request.setValue("text/event-stream", forHTTPHeaderField: "Accept")
                if let token = config.token {
                    request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
                }
                do {
                    let (bytes, response) = try await session.bytes(for: request)
                    guard let http = response as? HTTPURLResponse else {
                        throw DaemonError.unreachable
                    }
                    switch http.statusCode {
                    case 200: break
                    case 401: throw DaemonError.unauthorized
                    case let status: throw DaemonError.server(status: status)
                    }
                    var splitter = SSEByteSplitter()
                    var parser = SSELineParser()
                    for try await byte in bytes {
                        if let line = splitter.feed(byte),
                            let event = parser.feed(line)
                        {
                            continuation.yield(event)
                        }
                    }
                    continuation.finish()
                } catch {
                    continuation.finish(throwing: error)
                }
            }
            continuation.onTermination = { _ in task.cancel() }
        }
    }
}
