import { useEffect, useState } from "react";

import {
  disablePushNotifications,
  enablePushNotifications,
  readPushNotificationState,
  type PushNotificationState,
} from "@/lib/push-notifications";
import { setPushSubscriptionEndpoint } from "@/lib/backend-store";

function buttonLabel(
  state: PushNotificationState | null,
  busy: boolean,
): string {
  if (busy) {
    return "Notifications...";
  }

  switch (state?.type) {
    case "enabled":
      return "Notifications on";
    case "permissionDenied":
      return "Notifications blocked";
    case "error":
      return "Notifications error";
    default:
      return "Enable notifications";
  }
}

export function PushNotificationControl() {
  const [state, setState] = useState<PushNotificationState | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let cancelled = false;

    void readPushNotificationState().then(
      (nextState) => {
        if (!cancelled) {
          setState(nextState);
          if (nextState.type === "enabled") {
            void setPushSubscriptionEndpoint(nextState.endpoint);
          }
        }
      },
      (error: unknown) => {
        if (!cancelled) {
          setState({ type: "error", message: String(error) });
        }
      },
    );

    return () => {
      cancelled = true;
    };
  }, []);

  if (state?.type === "unsupported") {
    return null;
  }

  const disabled = busy || state?.type === "permissionDenied";
  const title = state?.type === "error" ? state.message : undefined;

  const handleClick = async () => {
    setBusy(true);
    try {
      const nextState =
        state?.type === "enabled"
          ? await disablePushNotifications()
          : await enablePushNotifications();
      setState(nextState);
      void setPushSubscriptionEndpoint(
        nextState.type === "enabled" ? nextState.endpoint : null,
      );
    } catch (error) {
      setState({ type: "error", message: String(error) });
    } finally {
      setBusy(false);
    }
  };

  return (
    <button
      className="notification-button"
      disabled={disabled}
      title={title}
      type="button"
      onClick={handleClick}
    >
      {buttonLabel(state, busy)}
    </button>
  );
}
