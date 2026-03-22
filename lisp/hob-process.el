;;; hob-process.el --- Subprocess lifecycle for hob -*- lexical-binding: t -*-

;;; Commentary:
;; Manages starting, stopping, and monitoring the hob-agent Rust subprocess.
;; Process output is fed to hob-ipc for parsing.

;;; Code:

(require 'json)

(defvar hob--process nil
  "The hob-agent subprocess, or nil if not running.")

(defvar hob--output-buffer ""
  "Accumulator for partial output lines from the subprocess.")

(defun hob-process-start ()
  "Start the hob-agent subprocess."
  (when (hob-process-running-p)
    (user-error "hob-agent is already running"))
  (unless (file-executable-p hob-agent-binary)
    (user-error "hob-agent binary not found at %s — run `make build'"
                hob-agent-binary))
  (setq hob--process
        (make-process
         :name "hob-agent"
         :buffer nil
         :command (list hob-agent-binary)
         :connection-type 'pipe
         :filter #'hob--process-filter
         :sentinel #'hob--process-sentinel
         :noquery t
         :env (list (concat "ANTHROPIC_API_KEY=" hob-api-key))))
  (message "hob-agent started (pid %d)" (process-id hob--process)))

(defun hob-process-stop ()
  "Stop the hob-agent subprocess."
  (when (hob-process-running-p)
    (delete-process hob--process)
    (setq hob--process nil)
    (message "hob-agent stopped")))

(defun hob-process-running-p ()
  "Return non-nil if the hob-agent subprocess is alive."
  (and hob--process (process-live-p hob--process)))

(defun hob-process-send (json-string)
  "Send JSON-STRING as a newline-terminated line to the subprocess."
  (unless (hob-process-running-p)
    (error "hob-agent is not running"))
  (process-send-string hob--process (concat json-string "\n")))

(defun hob--process-filter (process output)
  "Handle raw OUTPUT from PROCESS, splitting on newlines."
  (setq hob--output-buffer (concat hob--output-buffer output))
  (let ((lines (split-string hob--output-buffer "\n")))
    ;; All but the last element are complete lines
    (setq hob--output-buffer (car (last lines)))
    (dolist (line (butlast lines))
      (unless (string-blank-p line)
        (hob-ipc-dispatch line)))))

(defun hob--process-sentinel (process event)
  "Handle subprocess PROCESS lifecycle EVENT."
  (message "hob-agent: %s" (string-trim event))
  (unless (process-live-p process)
    (setq hob--process nil)))

(provide 'hob-process)
;;; hob-process.el ends here
