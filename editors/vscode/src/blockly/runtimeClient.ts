/**
 * Client for communicating with trust-runtime control endpoint
 * Sends I/O write commands for hardware execution from Blockly programs
 */

import * as net from "net";
import * as vscode from "vscode";

export interface RuntimeConfig {
  controlEndpoint: string; // e.g., "unix:///tmp/trust-debug.sock" or "tcp://127.0.0.1:9000"
  controlAuthToken?: string;
  requestTimeoutMs?: number;
}

export interface IoWriteParams {
  address: string; // IEC 61131-3 address (e.g., %QX0.0)
  value: any;
}

export interface ControlRequest {
  id: number;
  type: string;
  auth?: string;
  params?: any;
}

export interface ControlResponse {
  id: number;
  ok: boolean;
  result?: any;
  error?: string;
}

/**
 * Client for sending commands to trust-runtime control endpoint from Blockly
 */
export class RuntimeClient {
  private socket: net.Socket | null = null;
  private requestId = 1;
  private endpoint: string;
  private authToken?: string;
  private requestTimeoutMs: number;
  private buffer = "";
  private pendingRequests = new Map<number, { resolve: (value: any) => void; reject: (error: Error) => void }>();

  constructor(config: RuntimeConfig) {
    this.endpoint = config.controlEndpoint;
    this.authToken = config.controlAuthToken;
    this.requestTimeoutMs = config.requestTimeoutMs ?? 5000;
  }

  /**
   * Connect to the control endpoint
   */
  async connect(): Promise<void> {
    return new Promise((resolve, reject) => {
      try {
        // Parse endpoint
        if (this.endpoint.startsWith("tcp://")) {
          const address = this.endpoint.replace("tcp://", "");
          const [host, port] = address.split(":");
          this.socket = net.createConnection(
            { host, port: parseInt(port, 10) },
            () => {
              console.log("Blockly: Connected to trust-runtime via TCP:", address);
              resolve();
            }
          );
        } else if (this.endpoint.startsWith("unix://")) {
          const socketPath = this.endpoint.replace("unix://", "");
          this.socket = net.createConnection(socketPath, () => {
            console.log("Blockly: Connected to trust-runtime via Unix socket:", socketPath);
            resolve();
          });
        } else {
          reject(new Error(`Invalid control endpoint format: ${this.endpoint}`));
          return;
        }

        this.socket.on("data", (data) => {
          this.buffer += data.toString();
          this.processBuffer();
        });

        this.socket.on("error", (err) => {
          console.error("Blockly: Runtime connection error:", err);
          reject(err);
        });

        this.socket.on("close", () => {
          console.log("Blockly: Runtime connection closed");
          this.socket = null;
        });

      } catch (error) {
        reject(error);
      }
    });
  }

  /**
   * Process incoming data buffer
   */
  private processBuffer(): void {
    const lines = this.buffer.split("\n");
    this.buffer = lines.pop() || "";

    for (const line of lines) {
      if (!line.trim()) continue;
      
      try {
        const response: ControlResponse = JSON.parse(line);
        const pending = this.pendingRequests.get(response.id);
        
        if (pending) {
          this.pendingRequests.delete(response.id);
          if (response.ok) {
            pending.resolve(response.result);
          } else {
            pending.reject(new Error(response.error || "Unknown error"));
          }
        }
      } catch (error) {
        console.error("Blockly: Failed to parse response:", line, error);
      }
    }
  }

  /**
   * Disconnect from the control endpoint
   */
  disconnect(): void {
    if (this.socket) {
      this.socket.destroy();
      this.socket = null;
    }
    // Reject all pending requests
    for (const [id, { reject }] of this.pendingRequests) {
      reject(new Error("Connection closed"));
    }
    this.pendingRequests.clear();
  }

  /**
   * Write value to an I/O address (queued, may be overwritten by program)
   */
  async writeIo(address: string, value: any): Promise<void> {
    if (!this.socket) {
      throw new Error("Not connected to runtime");
    }

    const id = this.requestId++;
    const request: ControlRequest = {
      id,
      type: "io.force",  // Use force instead of write to persist across cycles
      auth: this.authToken,
      params: { address, value },
    };

    return new Promise((resolve, reject) => {
      this.pendingRequests.set(id, { resolve, reject });

      const timeout = setTimeout(() => {
        this.pendingRequests.delete(id);
        reject(new Error("Request timeout"));
      }, this.requestTimeoutMs);

      this.socket!.write(JSON.stringify(request) + "\n", (err) => {
        if (err) {
          clearTimeout(timeout);
          this.pendingRequests.delete(id);
          reject(err);
        }
      });
    });
  }

  /**
   * Resume runtime execution (ensure it's running PLC cycles)
   */
  async resume(): Promise<void> {
    if (!this.socket) {
      throw new Error("Not connected to runtime");
    }

    const id = this.requestId++;
    const request: ControlRequest = {
      id,
      type: "resume",
      auth: this.authToken,
    };

    return new Promise((resolve, reject) => {
      this.pendingRequests.set(id, { resolve, reject });

      const timeout = setTimeout(() => {
        this.pendingRequests.delete(id);
        reject(new Error("Request timeout"));
      }, this.requestTimeoutMs);

      this.socket!.write(JSON.stringify(request) + "\n", (err) => {
        if (err) {
          clearTimeout(timeout);
          this.pendingRequests.delete(id);
          reject(err);
        }
      });
    });
  }

  /**
   * Pause runtime execution
   */
  async pause(): Promise<void> {
    if (!this.socket) {
      throw new Error("Not connected to runtime");
    }

    const id = this.requestId++;
    const request: ControlRequest = {
      id,
      type: "pause",
      auth: this.authToken,
    };

    return new Promise((resolve, reject) => {
      this.pendingRequests.set(id, { resolve, reject });

      const timeout = setTimeout(() => {
        this.pendingRequests.delete(id);
        reject(new Error("Request timeout"));
      }, this.requestTimeoutMs);

      this.socket!.write(JSON.stringify(request) + "\n", (err) => {
        if (err) {
          clearTimeout(timeout);
          this.pendingRequests.delete(id);
          reject(err);
        }
      });
    });
  }

  /**
   * Check if connected to runtime
   */
  isConnected(): boolean {
    return this.socket !== null;
  }
}

/**
 * Get runtime configuration from workspace settings
 */
export function getRuntimeConfig(): RuntimeConfig {
  const config = vscode.workspace.getConfiguration("trust-lsp");
  
  return {
    controlEndpoint: config.get("runtime.controlEndpoint") || "unix:///tmp/trust-debug.sock",
    controlAuthToken: config.get("runtime.controlAuthToken"),
    requestTimeoutMs: config.get("runtime.requestTimeoutMs") || 5000,
  };
}
