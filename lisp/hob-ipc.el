;;; hob-ipc.el --- JSON IPC protocol for hob -*- lexical-binding: t -*-

;;; Commentary:
;; Encodes outgoing requests to and decodes incoming responses from hob-agent.
;; Dispatches parsed responses to hob-ui for rendering.

;;; Code:

(require 'json)
(require 'hob-process)

(defvar hob--task-counter 0
  "Monotonically increasing task ID counter.")

(defun hob-ipc--next-task-id ()
  "Generate a unique task ID string."
  (format "task-%d" (cl-incf hob--task-counter)))

(defun hob-ipc-send-task (prompt)
  "Send a task request with PROMPT to hob-agent."
  (let* ((id (hob-ipc--next-task-id))
         (msg (json-encode `(("type" . "task")
                             ("id"   . ,id)
                             ("prompt" . ,prompt)))))
    (hob-ui-task-started id prompt)
    (hob-process-send msg)
    id))

(defun hob-ipc-send-cancel (task-id)
  "Cancel in-progress task TASK-ID."
  (hob-process-send
   (json-encode `(("type" . "cancel") ("id" . ,task-id)))))

(defun hob-ipc-send-ping ()
  "Send a ping to check subprocess health."
  (hob-process-send (json-encode '(("type" . "ping")))))

(defun hob-ipc-dispatch (line)
  "Parse LINE as JSON and dispatch to the appropriate hob-ui handler."
  (condition-case err
      (let* ((msg (json-parse-string line :object-type 'alist))
             (type (alist-get 'type msg))
             (id   (alist-get 'id msg)))
        (pcase type
          ("token"
           (hob-ui-append-token id (alist-get 'content msg)))
          ("tool_call"
           (hob-ui-tool-call id
                             (alist-get 'tool msg)
                             (alist-get 'input msg)))
          ("tool_result"
           (hob-ui-tool-result id
                               (alist-get 'tool msg)
                               (alist-get 'output msg)))
          ("done"
           (hob-ui-task-done id))
          ("error"
           (hob-ui-task-error id (alist-get 'message msg)))
          ("status"
           (hob-ui-task-status id (alist-get 'message msg)))
          ("pong"
           (message "hob-agent: pong"))
          (_
           (message "hob-ipc: unknown message type: %s" type))))
    (json-parse-error
     (message "hob-ipc: failed to parse: %s" line))
    (error
     (message "hob-ipc: dispatch error: %s" err))))

(provide 'hob-ipc)
;;; hob-ipc.el ends here
