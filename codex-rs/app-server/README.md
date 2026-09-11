# Fork app-server API extensions

## Firehose observer

`event/firehose` marks the current initialized connection as a passive observer. Params may be omitted and the response is `{}`:

```json
{ "method": "event/firehose", "id": 17 }
{ "id": 17, "result": {} }
```

After subscribing, the connection receives one observed copy of each logical outbound app-server event. Thread-scoped notifications and requests are observed even when the thread has zero normal subscribers; ordinary connections without a thread subscription do not receive those events. Notifications are delivered with their original method and params. Server-initiated requests are delivered as `serverRequest/observed` notifications:

```json
{ "method": "serverRequest/observed", "params": {
    "request": { "method": "item/tool/requestUserInput", "id": 9, "params": { "...": "..." } }
} }
```

The observer connection cannot answer requests it only observed; JSON-RPC responses or errors for those ids are ignored. Firehose delivery ignores `optOutNotificationMethods`. The subscription does not add the connection to any thread subscriber set, does not replay pending thread requests, and does not keep single-client app-server mode alive by itself.

Subscription is idempotent and lasts until the connection closes. Explicit delivery to a connection takes precedence over observation, so a client that both participates and observes receives only one copy.

See [the feature contract](../../wf_features/app-server-firehose-subscription.md) for the behavior and validation expectations this fork preserves.
