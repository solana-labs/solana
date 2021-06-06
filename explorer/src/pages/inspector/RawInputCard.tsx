import React from "react";
import { Message } from "@solana/web3.js";
import type { TransactionData } from "./InspectorPage";
import { useQuery } from "utils/url";
import { useHistory, useLocation } from "react-router";

export function RawInput({
  value,
  setMessage,
}: {
  value?: string;
  setMessage: (param: TransactionData | undefined) => void;
}) {
  const rawTransactionInput = React.useRef<HTMLTextAreaElement>(null);
  const [error, setError] = React.useState();
  const [rows, setRows] = React.useState(3);
  const query = useQuery();
  const history = useHistory();
  const location = useLocation();

  const onInput = React.useCallback(() => {
    const base64 = rawTransactionInput.current?.value;
    if (base64) {
      if (query.get("message")) {
        query.delete("message");
        history.push({ ...location, search: query.toString() });
      }

      setRows(Math.max(3, Math.min(10, Math.round(base64.length / 150))));

      let buffer;
      try {
        buffer = Uint8Array.from(atob(base64), (c) => c.charCodeAt(0));
      } catch (err) {
        console.error(err);
        setError(err);
        return;
      }

      try {
        if (buffer.length < 36) {
          throw new Error("buffer is too short");
        }

        const message = Message.from(buffer);
        setMessage({
          rawMessage: buffer,
          message,
        });
        setError(undefined);
        return;
      } catch (err) {
        setError(err);
      }
    } else {
      setError(undefined);
    }
  }, [setMessage, history, query, location]);

  React.useEffect(() => {
    const input = rawTransactionInput.current;
    if (input && value) {
      input.value = value;
      onInput();
    }
  }, [value]); // eslint-disable-line react-hooks/exhaustive-deps

  const placeholder = "Paste raw base64 encoded transaction message";
  return (
    <div className="card">
      <div className="card-header">
        <h3 className="card-header-title">Encoded Transaction Message</h3>
      </div>
      <div className="card-body">
        <textarea
          rows={rows}
          onInput={onInput}
          ref={rawTransactionInput}
          className="form-control form-control-flush form-control-auto"
          placeholder={placeholder}
        ></textarea>
        <div className="row align-items-center">
          <div className="col d-flex align-items-center">
            {error && (
              <>
                <span className="text-warning small mr-2">
                  <i className="fe fe-alert-circle"></i>
                </span>

                <span className="text-warning">{(error as any).message}</span>
              </>
            )}
          </div>
        </div>
      </div>
      <div className="card-footer">
        <h3>Instructions</h3>
        <ul>
          <li className="mb-2">
            <strong>CLI: </strong>Use <code>--dump-transaction-message</code>{" "}
            flag
          </li>
          <li className="mb-2">
            <strong>Rust: </strong>Add <code>base64</code> crate dependency and{" "}
            <code>
              println!("{}", base64::encode(&transaction.message_data()));
            </code>
          </li>
          <li>
            <strong>JavaScript: </strong>Add{" "}
            <code>console.log(tx.serializeMessage().toString("base64"));</code>
          </li>
        </ul>
      </div>
    </div>
  );
}
