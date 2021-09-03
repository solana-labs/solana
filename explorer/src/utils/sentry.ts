import * as Sentry from "@sentry/react";

type Tags =
  | {
      [key: string]: string;
    }
  | undefined;

export function reportError(err: unknown, tags: Tags) {
  if (err instanceof Error) {
    console.error(err, err.message);
    try {
      Sentry.captureException(err, {
        tags,
      });
    } catch (err) {
      // Sentry can fail if error rate limit is reached
    }
  }
}
