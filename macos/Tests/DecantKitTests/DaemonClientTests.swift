import Foundation
import Testing

@testable import DecantKit

/// URLProtocol stub: scripts one response per request, keyed off nothing —
/// each test builds its own session so stubs never cross-talk.
final class StubProtocol: URLProtocol {
    nonisolated(unsafe) static var respond:
        (@Sendable (URLRequest) -> (Int, Data))? = nil

    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

    override func startLoading() {
        guard let respond = Self.respond, let url = request.url else {
            client?.urlProtocol(self, didFailWithError: URLError(.cannotConnectToHost))
            return
        }
        let (status, body) = respond(request)
        let response = HTTPURLResponse(
            url: url, statusCode: status, httpVersion: "HTTP/1.1", headerFields: nil)!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: body)
        client?.urlProtocolDidFinishLoading(self)
    }

    override func stopLoading() {}
}

@Suite("DaemonClient", .serialized)
struct DaemonClientTests {
    private func client(token: String? = "tok") -> DaemonClient {
        let cfg = URLSessionConfiguration.ephemeral
        cfg.protocolClasses = [StubProtocol.self]
        return DaemonClient(
            config: DaemonConfig(
                baseURL: URL(string: "http://127.0.0.1:4577")!, token: token),
            session: URLSession(configuration: cfg)
        )
    }

    @Test func successDecodesAndSendsBearer() async throws {
        StubProtocol.respond = { request in
            #expect(request.value(forHTTPHeaderField: "Authorization") == "Bearer tok")
            #expect(request.url?.path == "/api/v1/analytics/now")
            return (200, Data(ModelTests.nowJSON.utf8))
        }
        let now = try await client().now()
        #expect(now.today.sessions == 7)
    }

    @Test func statusMapping() async {
        for (status, expected) in [
            (401, DaemonError.unauthorized),
            (500, DaemonError.server(status: 500)),
            (404, DaemonError.server(status: 404)),
        ] {
            StubProtocol.respond = { _ in (status, Data()) }
            await #expect(throws: expected) { try await client().now() }
        }
    }

    @Test func garbageBodyIsDecodingError() async {
        StubProtocol.respond = { _ in (200, Data("not json".utf8)) }
        await #expect(throws: DaemonError.decoding) { try await client().now() }
    }

    @Test func connectionFailureIsUnreachable() async {
        StubProtocol.respond = nil
        await #expect(throws: DaemonError.unreachable) { try await client().now() }
    }
}
