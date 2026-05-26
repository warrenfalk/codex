# App-Server Firehose Subscription

## Intent

The app-server exposes a passive v2 event firehose for observer clients that need to maintain a broad live view of app-server activity without becoming a participant in any thread.

## Behavior

An initialized client subscribes by sending `event/firehose` with no params. The response is `{}`. The request is idempotent; sending it more than once leaves the connection subscribed. There is no unsubscribe method. A client stops observing by closing the connection.

After subscribing, the connection receives one observed copy of each logical outbound app-server event:

- broadcast server notifications,
- thread-scoped server notifications,
- targeted server notifications,
- server-initiated requests.

Server-initiated requests are not delivered to firehose observers as JSON-RPC requests. They are wrapped in a `serverRequest/observed` notification whose `request` field contains the original server request. This lets an observer display or log the request without being able to accidentally answer it.

Normal request ownership is preserved. A connection that only saw `serverRequest/observed` cannot resolve the real pending request by sending a JSON-RPC response or error with the observed request id. Connections that receive the real targeted request remain the only answerable owners.

Firehose delivery is deliberately independent of normal notification opt-outs. A connection that subscribed to the firehose receives observed notifications even if `initialize.params.capabilities.optOutNotificationMethods` would suppress the same method on the normal delivery path.

## Duplicate Avoidance

The firehose observes logical events, not per-subscriber writes. A thread notification sent to two normal thread subscribers produces one firehose notification. Broadcast notifications skip firehose subscribers on the normal broadcast path, so a firehose subscriber receives only the observed copy. If a connection is both a firehose subscriber and an explicit target of a notification or request, the explicit target delivery wins and the connection does not receive a second observed copy.

## Lifecycle Boundaries

Subscribing to the firehose does not subscribe the connection to any thread, does not replay pending thread requests, does not reset thread unload timers, and does not attach thread listeners by itself. A firehose-only connection does not count as a lifecycle connection for single-client app-server shutdown; if there are no normal clients and no running assistant turns, the app-server may exit even while a passive observer remains connected.

## Validation Expectations

Validation should cover protocol parsing for omitted `event/firehose` params, schema and TypeScript export generation, broadcast and targeted notification observation, thread event observation without thread subscription, duplicate avoidance with multiple thread subscribers, safe wrapping of server requests, and lifecycle behavior for firehose-only connections.
