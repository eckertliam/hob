;;; hob-test.el --- Tests for hob -*- lexical-binding: t -*-

;;; Commentary:
;; Unit and integration tests for the hob Elisp layer.
;; Run with: emacs --batch -L lisp/ -L test/ -l hob-test -f ert-run-tests-batch-and-exit

;;; Code:

(require 'ert)
(require 'cl-lib)
(require 'json)

;; Load hob modules
(require 'hob)
(require 'hob-process)
(require 'hob-ipc)
(require 'hob-ui)

;; ── hob--shell-getenv tests ────────────────────────────────────────

(ert-deftest hob-test-shell-getenv-reads-existing-var ()
  "hob--shell-getenv should read a variable that exists in the shell."
  ;; HOME is always set
  (let ((val (hob--shell-getenv "HOME")))
    (should (stringp val))
    (should (not (string-empty-p val)))))

(ert-deftest hob-test-shell-getenv-returns-nil-for-missing-var ()
  "hob--shell-getenv should return nil for an unset variable."
  (let ((val (hob--shell-getenv "HOB_NONEXISTENT_TEST_VAR_12345")))
    (should (null val))))

(ert-deftest hob-test-shell-getenv-no-trailing-whitespace ()
  "hob--shell-getenv should not include trailing newlines or spaces."
  (let ((val (hob--shell-getenv "HOME")))
    (when val
      (should (equal val (string-trim val))))))

(ert-deftest hob-test-shell-getenv-no-ansi-codes ()
  "hob--shell-getenv should not contain ANSI escape sequences."
  (let ((val (hob--shell-getenv "HOME")))
    (when val
      (should-not (string-match-p "\033\\[" val)))))

(ert-deftest hob-test-shell-getenv-matches-getenv ()
  "hob--shell-getenv should return the same value as getenv for HOME."
  (let ((shell-val (hob--shell-getenv "HOME"))
        (env-val (getenv "HOME")))
    (when (and shell-val env-val)
      (should (equal shell-val env-val)))))

;; ── API key resolution tests ───────────────────────────────────────

(ert-deftest hob-test-api-key-from-hob-api-key ()
  "hob-api-key takes highest priority."
  (let ((hob-api-key "sk-test-key")
        (process-environment (cons "ANTHROPIC_API_KEY=env-key" process-environment)))
    ;; The `or` chain in hob-process-start checks hob-api-key first
    (let ((resolved (or hob-api-key
                        (getenv "ANTHROPIC_API_KEY")
                        (getenv "OPENAI_API_KEY"))))
      (should (equal resolved "sk-test-key")))))

(ert-deftest hob-test-api-key-from-env ()
  "Falls back to ANTHROPIC_API_KEY env var when hob-api-key is nil."
  (let ((hob-api-key nil)
        (process-environment (cons "ANTHROPIC_API_KEY=env-key" process-environment)))
    (let ((resolved (or hob-api-key
                        (getenv "ANTHROPIC_API_KEY")
                        (getenv "OPENAI_API_KEY"))))
      (should (equal resolved "env-key")))))

(ert-deftest hob-test-api-key-openai-from-env ()
  "Falls back to OPENAI_API_KEY when ANTHROPIC_API_KEY is not set."
  (let ((hob-api-key nil)
        ;; Remove ANTHROPIC_API_KEY, add OPENAI_API_KEY
        (process-environment (cons "OPENAI_API_KEY=sk-openai"
                                   (cl-remove-if
                                    (lambda (s) (string-prefix-p "ANTHROPIC_API_KEY=" s))
                                    process-environment))))
    (let ((resolved (or hob-api-key
                        (getenv "ANTHROPIC_API_KEY")
                        (getenv "OPENAI_API_KEY"))))
      (should (equal resolved "sk-openai")))))

(ert-deftest hob-test-api-key-nil-when-nothing-set ()
  "Returns nil when no key is available anywhere."
  (let ((hob-api-key nil)
        (process-environment (cl-remove-if
                              (lambda (s) (or (string-prefix-p "ANTHROPIC_API_KEY=" s)
                                              (string-prefix-p "OPENAI_API_KEY=" s)))
                              process-environment)))
    (let ((resolved (or hob-api-key
                        (getenv "ANTHROPIC_API_KEY")
                        (getenv "OPENAI_API_KEY"))))
      (should (null resolved)))))

;; ── Process environment construction tests ─────────────────────────

(defun hob-test--build-process-env (api-key provider model openai-base-url)
  "Simulate the environment construction from hob-process-start.
Returns the environment list that would be prepended."
  (let ((hob-api-key api-key)
        (hob-provider provider)
        (hob-model model)
        (hob-openai-base-url openai-base-url))
    (append (list (concat "HOB_MODEL=" hob-model))
            (when hob-provider
              (list (concat "HOB_PROVIDER=" hob-provider)))
            (when api-key
              (cond
               ((equal hob-provider "openai")
                (list (concat "OPENAI_API_KEY=" api-key)))
               ((equal hob-provider "anthropic")
                (list (concat "ANTHROPIC_API_KEY=" api-key)))
               (t (list (concat "ANTHROPIC_API_KEY=" api-key)
                        (concat "OPENAI_API_KEY=" api-key)))))
            (when hob-openai-base-url
              (list (concat "OPENAI_API_BASE=" hob-openai-base-url))))))

(ert-deftest hob-test-env-anthropic-provider ()
  "Anthropic provider sets only ANTHROPIC_API_KEY."
  (let ((env (hob-test--build-process-env "sk-ant-test" "anthropic" "claude-sonnet-4-20250514" nil)))
    (should (member "ANTHROPIC_API_KEY=sk-ant-test" env))
    (should (member "HOB_PROVIDER=anthropic" env))
    (should (member "HOB_MODEL=claude-sonnet-4-20250514" env))
    (should-not (cl-find-if (lambda (s) (string-prefix-p "OPENAI_API_KEY=" s)) env))))

(ert-deftest hob-test-env-openai-provider ()
  "OpenAI provider sets only OPENAI_API_KEY."
  (let ((env (hob-test--build-process-env "sk-openai" "openai" "gpt-4o" nil)))
    (should (member "OPENAI_API_KEY=sk-openai" env))
    (should (member "HOB_PROVIDER=openai" env))
    (should-not (cl-find-if (lambda (s) (string-prefix-p "ANTHROPIC_API_KEY=" s)) env))))

(ert-deftest hob-test-env-auto-detect-sets-both ()
  "Auto-detect (nil provider) sets both API key env vars."
  (let ((env (hob-test--build-process-env "sk-test" nil "claude-sonnet-4-20250514" nil)))
    (should (member "ANTHROPIC_API_KEY=sk-test" env))
    (should (member "OPENAI_API_KEY=sk-test" env))
    (should-not (cl-find-if (lambda (s) (string-prefix-p "HOB_PROVIDER=" s)) env))))

(ert-deftest hob-test-env-no-key-no-key-vars ()
  "No API key means no API key env vars are set."
  (let ((env (hob-test--build-process-env nil nil "claude-sonnet-4-20250514" nil)))
    (should-not (cl-find-if (lambda (s) (string-prefix-p "ANTHROPIC_API_KEY=" s)) env))
    (should-not (cl-find-if (lambda (s) (string-prefix-p "OPENAI_API_KEY=" s)) env))))

(ert-deftest hob-test-env-openai-base-url ()
  "Custom base URL is passed through."
  (let ((env (hob-test--build-process-env "sk-test" "openai" "gpt-4o" "http://localhost:11434")))
    (should (member "OPENAI_API_BASE=http://localhost:11434" env))))

(ert-deftest hob-test-env-no-base-url-by-default ()
  "No OPENAI_API_BASE when hob-openai-base-url is nil."
  (let ((env (hob-test--build-process-env "sk-test" nil "gpt-4o" nil)))
    (should-not (cl-find-if (lambda (s) (string-prefix-p "OPENAI_API_BASE=" s)) env))))

;; ── IPC dispatch tests ─────────────────────────────────────────────

(ert-deftest hob-test-ipc-dispatch-token ()
  "Dispatch a token message to hob-ui-append-token."
  (let ((received-content nil))
    (cl-letf (((symbol-function 'hob-ui-append-token)
               (lambda (_id content) (setq received-content content))))
      (hob-ipc-dispatch "{\"type\":\"token\",\"id\":\"t1\",\"content\":\"hello\"}")
      (should (equal received-content "hello")))))

(ert-deftest hob-test-ipc-dispatch-done ()
  "Dispatch a done message to hob-ui-task-done."
  (let ((received-id nil))
    (cl-letf (((symbol-function 'hob-ui-task-done)
               (lambda (id) (setq received-id id))))
      (hob-ipc-dispatch "{\"type\":\"done\",\"id\":\"t1\"}")
      (should (equal received-id "t1")))))

(ert-deftest hob-test-ipc-dispatch-error ()
  "Dispatch an error message to hob-ui-task-error."
  (let (received-id received-msg)
    (cl-letf (((symbol-function 'hob-ui-task-error)
               (lambda (id msg) (setq received-id id received-msg msg))))
      (hob-ipc-dispatch "{\"type\":\"error\",\"id\":\"t1\",\"message\":\"boom\"}")
      (should (equal received-id "t1"))
      (should (equal received-msg "boom")))))

(ert-deftest hob-test-ipc-dispatch-tool-call ()
  "Dispatch a tool_call message."
  (let (received-tool)
    (cl-letf (((symbol-function 'hob-ui-tool-call)
               (lambda (_id tool _input) (setq received-tool tool))))
      (hob-ipc-dispatch "{\"type\":\"tool_call\",\"id\":\"t1\",\"tool\":\"read_file\",\"input\":{}}")
      (should (equal received-tool "read_file")))))

(ert-deftest hob-test-ipc-dispatch-tool-result ()
  "Dispatch a tool_result message."
  (let (received-output)
    (cl-letf (((symbol-function 'hob-ui-tool-result)
               (lambda (_id _tool output) (setq received-output output))))
      (hob-ipc-dispatch "{\"type\":\"tool_result\",\"id\":\"t1\",\"tool\":\"read_file\",\"output\":\"file contents\"}")
      (should (equal received-output "file contents")))))

(ert-deftest hob-test-ipc-dispatch-status ()
  "Dispatch a status message."
  (let (received-msg)
    (cl-letf (((symbol-function 'hob-ui-task-status)
               (lambda (_id msg) (setq received-msg msg))))
      (hob-ipc-dispatch "{\"type\":\"status\",\"id\":\"t1\",\"message\":\"retrying in 4s\"}")
      (should (equal received-msg "retrying in 4s")))))

(ert-deftest hob-test-ipc-dispatch-pong ()
  "Dispatch a pong message without error."
  ;; Should not error, just message
  (hob-ipc-dispatch "{\"type\":\"pong\"}"))

(ert-deftest hob-test-ipc-dispatch-invalid-json ()
  "Invalid JSON doesn't throw, just messages."
  ;; Should not error
  (hob-ipc-dispatch "this is not json"))

(ert-deftest hob-test-ipc-dispatch-unknown-type ()
  "Unknown message type doesn't throw."
  (hob-ipc-dispatch "{\"type\":\"unknown_thing\",\"id\":\"t1\"}"))

;; ── IPC encoding tests ─────────────────────────────────────────────

(ert-deftest hob-test-ipc-task-id-increments ()
  "Task IDs increment monotonically."
  (let ((hob--task-counter 0))
    (should (equal (hob-ipc--next-task-id) "task-1"))
    (should (equal (hob-ipc--next-task-id) "task-2"))
    (should (equal (hob-ipc--next-task-id) "task-3"))))

;; ── Process filter tests ───────────────────────────────────────────

(ert-deftest hob-test-process-filter-complete-line ()
  "Complete JSON lines are dispatched."
  (let ((hob--output-buffer "")
        (dispatched nil))
    (cl-letf (((symbol-function 'hob-ipc-dispatch)
               (lambda (line) (push line dispatched))))
      (hob--process-filter nil "{\"type\":\"pong\"}\n")
      (should (equal (car dispatched) "{\"type\":\"pong\"}"))
      (should (equal hob--output-buffer "")))))

(ert-deftest hob-test-process-filter-partial-then-complete ()
  "Partial lines are buffered until complete."
  (let ((hob--output-buffer "")
        (dispatched nil))
    (cl-letf (((symbol-function 'hob-ipc-dispatch)
               (lambda (line) (push line dispatched))))
      ;; First chunk: partial line
      (hob--process-filter nil "{\"type\":")
      (should (null dispatched))
      (should (equal hob--output-buffer "{\"type\":"))
      ;; Second chunk: completes the line
      (hob--process-filter nil "\"pong\"}\n")
      (should (equal (car dispatched) "{\"type\":\"pong\"}"))
      (should (equal hob--output-buffer "")))))

(ert-deftest hob-test-process-filter-multiple-lines ()
  "Multiple lines in one chunk are all dispatched."
  (let ((hob--output-buffer "")
        (dispatched nil))
    (cl-letf (((symbol-function 'hob-ipc-dispatch)
               (lambda (line) (push line dispatched))))
      (hob--process-filter nil "{\"a\":1}\n{\"b\":2}\n")
      (should (= (length dispatched) 2)))))

(ert-deftest hob-test-process-filter-blank-lines-skipped ()
  "Blank lines are not dispatched."
  (let ((hob--output-buffer "")
        (dispatched nil))
    (cl-letf (((symbol-function 'hob-ipc-dispatch)
               (lambda (line) (push line dispatched))))
      (hob--process-filter nil "\n\n{\"a\":1}\n\n")
      (should (= (length dispatched) 1)))))

;; ── UI buffer tests ────────────────────────────────────────────────

(ert-deftest hob-test-ui-buffer-created ()
  "hob-ui--get-or-create-buffer creates the buffer."
  (when (get-buffer "*hob*")
    (kill-buffer "*hob*"))
  (let ((buf (hob-ui--get-or-create-buffer)))
    (unwind-protect
        (progn
          (should (bufferp buf))
          (should (equal (buffer-name buf) "*hob*"))
          (with-current-buffer buf
            (should (eq major-mode 'hob-chat-mode))
            ;; Input marker should exist
            (should (markerp hob--input-marker))
            ;; Output end marker should exist
            (should (markerp hob--output-end))))
      (kill-buffer buf))))

(ert-deftest hob-test-ui-input-area-editable ()
  "Text can be inserted in the input area."
  (when (get-buffer "*hob*")
    (kill-buffer "*hob*"))
  (let ((buf (hob-ui--get-or-create-buffer)))
    (unwind-protect
        (with-current-buffer buf
          (goto-char (point-max))
          (insert "test input")
          (should (equal (hob-ui--input-text) "test input")))
      (kill-buffer buf))))

(ert-deftest hob-test-ui-clear-input ()
  "hob-ui--clear-input removes text from the input area."
  (when (get-buffer "*hob*")
    (kill-buffer "*hob*"))
  (let ((buf (hob-ui--get-or-create-buffer)))
    (unwind-protect
        (with-current-buffer buf
          (goto-char (point-max))
          (insert "test input")
          (hob-ui--clear-input)
          (should (equal (hob-ui--input-text) "")))
      (kill-buffer buf))))

(ert-deftest hob-test-ui-append-output ()
  "hob-ui--append-output adds text to the history region."
  (when (get-buffer "*hob*")
    (kill-buffer "*hob*"))
  (let ((buf (hob-ui--get-or-create-buffer)))
    (unwind-protect
        (with-current-buffer buf
          (hob-ui--append-output "hello world")
          (goto-char (point-min))
          (should (search-forward "hello world" nil t)))
      (kill-buffer buf))))

(ert-deftest hob-test-ui-modeline-idle ()
  "Modeline shows idle by default."
  (when (get-buffer "*hob*")
    (kill-buffer "*hob*"))
  (let ((buf (hob-ui--get-or-create-buffer)))
    (unwind-protect
        (with-current-buffer buf
          (should (string-match-p "idle" (hob-ui--modeline-string))))
      (kill-buffer buf))))

(ert-deftest hob-test-ui-modeline-streaming ()
  "Modeline shows streaming when set."
  (when (get-buffer "*hob*")
    (kill-buffer "*hob*"))
  (let ((buf (hob-ui--get-or-create-buffer)))
    (unwind-protect
        (with-current-buffer buf
          (hob-ui--set-status "streaming")
          (should (string-match-p "streaming" (hob-ui--modeline-string))))
      (kill-buffer buf))))

;; ── Integration: mock subprocess ───────────────────────────────────

(ert-deftest hob-test-integration-ping-pong ()
  "Send ping via a mock cat process and verify pong dispatch."
  (when (get-buffer "*hob*")
    (kill-buffer "*hob*"))
  (let ((hob--process nil)
        (hob--output-buffer "")
        (pong-received nil))
    ;; Start a cat process that echoes back
    (setq hob--process
          (make-process
           :name "hob-test-mock"
           :buffer nil
           :command (list "cat")
           :connection-type 'pipe
           :filter #'hob--process-filter
           :noquery t))
    (unwind-protect
        (progn
          ;; Mock the pong handler
          (cl-letf (((symbol-function 'hob-ipc-dispatch)
                     (lambda (line)
                       (when (string-match-p "pong" line)
                         (setq pong-received t)))))
            ;; Send something that cat will echo back
            (process-send-string hob--process "{\"type\":\"pong\"}\n")
            ;; Give it a moment to echo
            (sleep-for 0.1)
            (accept-process-output hob--process 1)
            (should pong-received)))
      (when (process-live-p hob--process)
        (delete-process hob--process)))))

(provide 'hob-test)
;;; hob-test.el ends here
