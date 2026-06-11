import Foundation

public enum DaemonError: Error, Equatable, Sendable {
    /// Connection refused / timed out — the daemon is probably not running.
    case unreachable
    /// 401: token missing or stale.
    case unauthorized
    /// Daemon answered but with a non-2xx status (it's up, something's wrong).
    case server(status: Int)
    /// 2xx body that didn't match the contract.
    case decoding
}

/// What `NowStore` needs from the network; `DaemonClient` is the real one,
/// tests inject fakes.
public protocol NowProviding: Sendable {
    func now() async throws -> Now
}

/// Thin typed client for the daemon's loopback API.
public struct DaemonClient: NowProviding {
    private static let decoder = JSONDecoder()

    private let config: DaemonConfig
    private let session: URLSession

    public init(config: DaemonConfig, session: URLSession = .shared) {
        self.config = config
        self.session = session
    }

    public func now() async throws -> Now {
        try await get(Envelope<Now>.self, path: "api/v1/analytics/now").data
    }

    private func get<T: Decodable>(_ type: T.Type, path: String) async throws -> T {
        var request = URLRequest(url: config.baseURL.appendingPathComponent(path))
        request.timeoutInterval = 5
        if let token = config.token {
            request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        }

        let (data, response): (Data, URLResponse)
        do {
            (data, response) = try await session.data(for: request)
        } catch {
            throw DaemonError.unreachable
        }
        guard let http = response as? HTTPURLResponse else { throw DaemonError.unreachable }
        switch http.statusCode {
        case 200..<300: break
        case 401: throw DaemonError.unauthorized
        case let status: throw DaemonError.server(status: status)
        }
        do {
            return try Self.decoder.decode(type, from: data)
        } catch {
            throw DaemonError.decoding
        }
    }
}
