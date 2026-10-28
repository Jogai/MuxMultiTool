import { createClient } from "@connectrpc/connect";
import { createConnectTransport } from "@connectrpc/connect-web";

import { SystemService } from "~/generated/muxmultitool/system/v1/system_pb";

/**
 * Creates the Connect transport for the backend under `/rpc`.
 * Methods marked `NO_SIDE_EFFECTS` in the proto files are sent as GET requests.
 * JSON in development so devtools can read the traffic, binary protobuf otherwise.
 */
export function createRpcTransport(baseUrl = "/rpc") {
	return createConnectTransport({
		baseUrl,
		useBinaryFormat: !import.meta.env.DEV,
		useHttpGet: true
	});
}

/** Client for the system service (health and version). */
export function createSystemClient(transport = createRpcTransport()) {
	return createClient(SystemService, transport);
}
