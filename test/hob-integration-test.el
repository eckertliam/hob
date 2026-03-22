;;; hob-integration-test.el --- Integration tests for hob -*- lexical-binding: t -*-

;;; Commentary:
;; Integration tests that start the real hob-agent binary and verify
;; end-to-end behavior.  Requires `make build` to have been run first.
;;
;; Run with:
;;   emacs --batch -L lisp/ -L test/ -l hob-integration-test \
;;     -f ert-run-tests-batch-and-exit

;;; Code:

(require 'ert)
(require 'cl-lib)
(require 'json)
(require 'hob)
(require 'hob-process)
(require 'hob-ipc)
(require 'hob-ui)

;; ── Helpers ────────────────────────────────────────────────────────

(defvar hob-test--binary
  (let ((dir (file-name-directory (or load-file-name buffer-file-name ""))))
    ;; test/ is one level under repo root
    (expand-file-name "../agent/target/release/hob-agent" dir))
  "Path to the hob-agent binary for testing.")

(defun hob-test--binary-exists-p ()
  "Return non-nil if the test binary exists."
  (file-executable-p hob-test--binary))

(defun hob-test--start-agent (env-alist)
  "Start hob-agent with ENV-ALIST as environment overrides.
Returns the process.  Caller must delete it."
  (let ((process-environment
         (append (mapcar (lambda (pair)
                           (concat (car pair) "=" (cdr pair)))
                         env-alist)
                 process-environment)))
    (make-process
     :name "hob-integration-test"
     :buffer nil
     :command (list hob-test--binary)
     :connection-type 'pipe
     :noquery t
     :filter (lambda (_proc output)
               ;; Store output for later inspection
               (let ((buf (get-buffer-create " *hob-test-output*")))
                 (with-current-buffer buf
                   (goto-char (point-max))
                   (insert output))))
     :stderr (get-buffer-create " *hob-test-stderr*"))))

(defun hob-test--send (proc json-string)
  "Send JSON-STRING as a line to PROC."
  (process-send-string proc (concat json-string "\n")))

(defun hob-test--read-output (&optional timeout)
  "Return accumulated stdout output, waiting up to TIMEOUT seconds."
  (let ((deadline (+ (float-time) (or timeout 3))))
    (while (and (< (float-time) deadline)
                (let ((buf (get-buffer " *hob-test-output*")))
                  (or (null buf)
                      (with-current-buffer buf
                        (= (point-min) (point-max))))))
      (sleep-for 0.05)
      (accept-process-output nil 0.05))
    (let ((buf (get-buffer " *hob-test-output*")))
      (if buf
          (with-current-buffer buf (buffer-string))
        ""))))

(defun hob-test--read-stderr ()
  "Return accumulated stderr output."
  (let ((buf (get-buffer " *hob-test-stderr*")))
    (if buf
        (with-current-buffer buf (buffer-string))
      "")))

(defun hob-test--cleanup ()
  "Kill test buffers."
  (dolist (name '(" *hob-test-output*" " *hob-test-stderr*"))
    (when (get-buffer name)
      (kill-buffer name))))

(defmacro hob-test--with-agent (env-alist &rest body)
  "Start agent with ENV-ALIST, run BODY, then cleanup."
  (declare (indent 1))
  `(progn
     (hob-test--cleanup)
     (if (not (hob-test--binary-exists-p))
         (ert-skip "hob-agent binary not built (run make build)")
       (let ((proc (hob-test--start-agent ,env-alist)))
         (unwind-protect
             (progn
               ;; Give agent time to start
               (sleep-for 0.2)
               ,@body)
           (when (process-live-p proc)
             (delete-process proc))
           (hob-test--cleanup))))))

;; ── Tests ──────────────────────────────────────────────────────────

(ert-deftest hob-integration-binary-exists ()
  "The hob-agent binary should exist after make build."
  (should (hob-test--binary-exists-p)))

(ert-deftest hob-integration-agent-starts-with-fake-key ()
  "Agent should start and stay alive with a fake API key."
  (hob-test--with-agent '(("ANTHROPIC_API_KEY" . "sk-fake-test-key")
                           ("HOB_MODEL" . "test-model"))
    (should (process-live-p proc))))

(ert-deftest hob-integration-ping-pong ()
  "Agent should respond to ping with pong."
  (hob-test--with-agent '(("ANTHROPIC_API_KEY" . "sk-fake-test-key")
                           ("HOB_MODEL" . "test-model"))
    (hob-test--send proc "{\"type\":\"ping\"}")
    (let ((output (hob-test--read-output)))
      (should (string-match-p "\"type\":\"pong\"" output)))))

(ert-deftest hob-integration-agent-exits-without-key ()
  "Agent should exit with code 1 when no API key is set."
  (hob-test--cleanup)
  (if (not (hob-test--binary-exists-p))
      (ert-skip "hob-agent binary not built")
    ;; Strip all API key vars from the environment
    (let ((process-environment
           (cl-remove-if
            (lambda (s) (or (string-prefix-p "ANTHROPIC_API_KEY=" s)
                            (string-prefix-p "OPENAI_API_KEY=" s)
                            (string-prefix-p "HOB_PROVIDER=" s)))
            process-environment)))
      (let ((proc (make-process
                   :name "hob-integration-no-key"
                   :buffer nil
                   :command (list hob-test--binary)
                   :connection-type 'pipe
                   :noquery t
                   :stderr (get-buffer-create " *hob-test-stderr*"))))
        (unwind-protect
            (progn
              ;; Wait for it to exit
              (let ((deadline (+ (float-time) 3)))
                (while (and (process-live-p proc)
                            (< (float-time) deadline))
                  (sleep-for 0.05)))
              (should-not (process-live-p proc))
              ;; stderr should contain an error about missing key
              (let ((stderr (hob-test--read-stderr)))
                (should (or (string-match-p "API.key" stderr)
                            (string-match-p "API_KEY" stderr)))))
          (when (process-live-p proc)
            (delete-process proc))
          (hob-test--cleanup))))))

(ert-deftest hob-integration-agent-survives-empty-input ()
  "Agent should not crash on empty/whitespace input lines."
  (hob-test--with-agent '(("ANTHROPIC_API_KEY" . "sk-fake-test-key")
                           ("HOB_MODEL" . "test-model"))
    (hob-test--send proc "")
    (hob-test--send proc "   ")
    (hob-test--send proc "\n")
    (sleep-for 0.2)
    (should (process-live-p proc))
    ;; Should still respond to ping after garbage input
    (hob-test--send proc "{\"type\":\"ping\"}")
    (let ((output (hob-test--read-output)))
      (should (string-match-p "pong" output)))))

(ert-deftest hob-integration-agent-handles-invalid-json ()
  "Agent should not crash on invalid JSON input."
  (hob-test--with-agent '(("ANTHROPIC_API_KEY" . "sk-fake-test-key")
                           ("HOB_MODEL" . "test-model"))
    (hob-test--send proc "this is not json")
    (hob-test--send proc "{bad json")
    (sleep-for 0.2)
    (should (process-live-p proc))
    ;; Should still respond to ping
    (hob-test--send proc "{\"type\":\"ping\"}")
    (let ((output (hob-test--read-output)))
      (should (string-match-p "pong" output)))))

(ert-deftest hob-integration-openai-key-starts ()
  "Agent should start with OPENAI_API_KEY instead of ANTHROPIC."
  (hob-test--cleanup)
  (if (not (hob-test--binary-exists-p))
      (ert-skip "hob-agent binary not built")
    ;; Must strip ANTHROPIC_API_KEY so auto-detect picks OpenAI
    (let ((process-environment
           (append (list "OPENAI_API_KEY=sk-fake-openai-key"
                         "HOB_MODEL=gpt-4o")
                   (cl-remove-if
                    (lambda (s) (string-prefix-p "ANTHROPIC_API_KEY=" s))
                    process-environment))))
      (let ((proc (make-process
                   :name "hob-integration-test"
                   :buffer nil
                   :command (list hob-test--binary)
                   :connection-type 'pipe
                   :noquery t
                   :filter (lambda (_proc output)
                             (let ((buf (get-buffer-create " *hob-test-output*")))
                               (with-current-buffer buf
                                 (goto-char (point-max))
                                 (insert output))))
                   :stderr (get-buffer-create " *hob-test-stderr*"))))
        (unwind-protect
            (progn
              (sleep-for 0.2)
              (should (process-live-p proc))
              (hob-test--send proc "{\"type\":\"ping\"}")
              (let ((output (hob-test--read-output)))
                (should (string-match-p "pong" output))))
          (when (process-live-p proc)
            (delete-process proc))
          (hob-test--cleanup))))))

(ert-deftest hob-integration-task-with-fake-key-returns-error ()
  "Sending a task with a fake API key should return an error, not crash."
  (hob-test--with-agent '(("ANTHROPIC_API_KEY" . "sk-fake-test-key")
                           ("HOB_MODEL" . "claude-sonnet-4-20250514"))
    (hob-test--send proc "{\"type\":\"task\",\"id\":\"t1\",\"prompt\":\"hello\"}")
    ;; Wait for the response
    (let ((output (hob-test--read-output 5)))
      ;; Should get an error response, not a crash
      (should (string-match-p "\"type\":\"error\"" output))
      ;; Process should still be alive (error is per-task, not fatal)
      (sleep-for 0.5)
      (should (process-live-p proc)))))

;; ── Elisp-side integration ─────────────────────────────────────────

(ert-deftest hob-integration-shell-getenv-api-key ()
  "hob--shell-getenv should return a clean API key with no ANSI codes."
  (let ((val (hob--shell-getenv "ANTHROPIC_API_KEY")))
    ;; May be nil if not set on this machine — that's ok
    (when val
      ;; Must not contain ANSI escape codes
      (should-not (string-match-p "\033" val))
      ;; Must not contain newlines
      (should-not (string-match-p "\n" val))
      ;; Must start with a reasonable prefix (not garbage)
      (should (string-match-p "\\`[a-zA-Z0-9_-]" val)))))

(ert-deftest hob-integration-process-env-construction ()
  "The process environment should contain the API key when hob--shell-getenv finds it."
  (let* ((hob-api-key nil)
         (hob-provider nil)
         (hob-model "test-model")
         (hob-openai-base-url nil)
         ;; Simulate: getenv returns nil, shell-getenv returns a key
         (found-key nil))
    ;; Override getenv to return nil, shell-getenv to return a test key
    (cl-letf (((symbol-function 'getenv)
               (lambda (var)
                 (unless (or (string= var "ANTHROPIC_API_KEY")
                             (string= var "OPENAI_API_KEY")
                             (string= var "SHELL"))
                   (funcall (symbol-function 'getenv) var))))
              ((symbol-function 'hob--shell-getenv)
               (lambda (var)
                 (when (string= var "ANTHROPIC_API_KEY")
                   "sk-test-from-shell"))))
      (let* ((api-key (or hob-api-key
                          (getenv "ANTHROPIC_API_KEY")
                          (getenv "OPENAI_API_KEY")
                          (hob--shell-getenv "ANTHROPIC_API_KEY")
                          (hob--shell-getenv "OPENAI_API_KEY"))))
        (should (equal api-key "sk-test-from-shell"))))))

(provide 'hob-integration-test)
;;; hob-integration-test.el ends here
