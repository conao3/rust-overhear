import { createClient } from "graphql-ws";
import WebSocket from "ws";
import { readFileSync, openSync, writeSync, closeSync } from "node:fs";
import { parseArgs } from "node:util";

const { values } = parseArgs({
  options: {
    connection: { type: "string" },
    output: { type: "string" },
    seconds: { type: "string" },
    help: { type: "boolean" },
  },
});
if (values.help) {
  console.log(
    "node scripts/record-segments.mjs --connection FILE --output FILE --seconds N",
  );
  process.exit(0);
}
if (!values.connection || !values.output || !values.seconds) {
  throw new Error("--connection, --output and --seconds are required");
}
const seconds = Number(values.seconds);
if (!Number.isFinite(seconds) || seconds <= 0 || seconds > 86400) {
  throw new Error("--seconds must be greater than 0 and at most 86400");
}
const connection = JSON.parse(readFileSync(values.connection, "utf8"));
if (
  !/^wss?:\/\//.test(connection.websocket) ||
  typeof connection.token !== "string" ||
  !connection.token
) {
  throw new Error(
    "The connection file must contain websocket and a non-empty token",
  );
}
const output = openSync(values.output, "wx", 0o600);
const start = performance.now();
const client = createClient({
  url: connection.websocket,
  webSocketImpl: WebSocket,
  connectionParams: { token: connection.token },
  connectionAckWaitTimeout: 5000,
  retryAttempts: 0,
});
let count = 0;
let timer;
try {
  await new Promise((resolve, reject) => {
    timer = setTimeout(
      () =>
        count > 0
          ? resolve()
          : reject(new Error("No segment updates received")),
      seconds * 1000,
    );
    client.subscribe(
      {
        query:
          "subscription { segmentUpdates { id status startMs endMs asrEngine sourceText tokens { surface } translations { engineId text } } }",
      },
      {
        next(result) {
          if (result.errors || !result.data?.segmentUpdates) {
            reject(
              new Error(
                JSON.stringify(result.errors ?? "Missing segmentUpdates"),
              ),
            );
            return;
          }
          try {
            writeSync(
              output,
              JSON.stringify({
                t: performance.now() - start,
                ...result.data.segmentUpdates,
              }) + "\n",
            );
            count += 1;
          } catch (error) {
            reject(error);
          }
        },
        error(error) {
          reject(
            new Error(error?.reason || error?.message || JSON.stringify(error)),
          );
        },
        complete() {
          reject(new Error("Subscription ended before the recording duration"));
        },
      },
    );
  });
  console.log(`Recorded ${count} updates`);
} finally {
  clearTimeout(timer);
  await client.dispose();
  closeSync(output);
}
