import type { DescService } from "@bufbuild/protobuf";
import { type Client, createClient, type Transport } from "@connectrpc/connect";
import { createConnectTransport } from "@connectrpc/connect-web";

export interface RpcTransportOptions {
	/**
	 * Where the Connect router is mounted. A path without origin, so the request goes to the page's own
	 * origin: the axum server in the container, the embedded server in the desktop app, and the Vite proxy
	 * in development. Nothing needs configuring.
	 */
	baseUrl?: string;
	/** Fetch override, used by tests to capture the requests. */
	fetch?: typeof globalThis.fetch;
}

/**
 * Creates the Connect transport for the backend under `/rpc`.
 * Methods marked `NO_SIDE_EFFECTS` in the proto files are sent as GET requests.
 * JSON in development so devtools can read the traffic, binary protobuf otherwise.
 */
export function createRpcTransport({ baseUrl = "/rpc", fetch }: RpcTransportOptions = {}): Transport {
	return createConnectTransport({
		baseUrl,
		useBinaryFormat: !import.meta.env.DEV,
		useHttpGet: true,
		fetch
	});
}

let sharedTransport: Transport | undefined;
const clients = new Map<DescService, Client<DescService>>();

/** The one transport the whole app shares; created on first use. */
export function getRpcTransport(): Transport {
	sharedTransport ??= createRpcTransport();
	return sharedTransport;
}

/**
 * Replaces the shared transport and forgets the clients built on it.
 * Tests use it to inject a fake fetch; `undefined` restores the default.
 */
export function setRpcTransport(transport: Transport | undefined): void {
	sharedTransport = transport;
	clients.clear();
}

/**
 * The typed client for a generated service, e.g. `useRpcClient(SystemService)`.
 * One client per service, all on the shared transport, so every feature talks to the same backend.
 */
export function useRpcClient<T extends DescService>(service: T): Client<T> {
	let client = clients.get(service) as Client<T> | undefined;
	if (!client) {
		client = createClient(service, getRpcTransport());
		clients.set(service, client);
	}
	return client;
}
