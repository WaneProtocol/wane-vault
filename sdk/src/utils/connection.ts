import { Connection, Commitment } from "@solana/web3.js";

/**
 * Health status for an RPC endpoint.
 */
export interface EndpointHealth {
  url: string;
  healthy: boolean;
  latencyMs: number;
  lastChecked: number;
  consecutiveFailures: number;
  slot?: number;
}

/**
 * Rate limiter state per endpoint.
 */
interface RateLimiterState {
  tokens: number;
  maxTokens: number;
  refillRate: number; // tokens per second
  lastRefill: number;
}

/**
 * Configuration for the ConnectionManager.
 */
export interface ConnectionManagerConfig {
  /** RPC endpoint URLs in priority order */
  endpoints: string[];
  /** Commitment level for connections */
  commitment: Commitment;
  /** Health check interval in ms */
  healthCheckIntervalMs: number;
  /** Maximum consecutive failures before marking unhealthy */
  maxConsecutiveFailures: number;
  /** Request timeout in ms */
  requestTimeoutMs: number;
  /** Maximum requests per second per endpoint */
  maxRequestsPerSecond: number;
  /** Whether to enable automatic failover */
  autoFailover: boolean;
  /** Whether to run health checks automatically */
  autoHealthCheck: boolean;
}

const DEFAULT_CONNECTION_CONFIG: ConnectionManagerConfig = {
  endpoints: ["https://api.mainnet-beta.solana.com"],
  commitment: "confirmed",
  healthCheckIntervalMs: 30_000,
  maxConsecutiveFailures: 3,
  requestTimeoutMs: 30_000,
  maxRequestsPerSecond: 10,
  autoFailover: true,
  autoHealthCheck: true,
};

/**
 * Manages multiple RPC connections with health checking,
 * automatic failover, and rate limiting.
 *
 * Provides a single Connection interface that transparently
 * routes requests to the healthiest available endpoint.
 */
export class ConnectionManager {
  private connections: Map<string, Connection> = new Map();
  private healthStatus: Map<string, EndpointHealth> = new Map();
  private rateLimiters: Map<string, RateLimiterState> = new Map();
  private config: ConnectionManagerConfig;
  private activeEndpoint: string;
  private healthCheckTimer: ReturnType<typeof setInterval> | null = null;
  private requestCounts: Map<string, number[]> = new Map();

  constructor(config?: Partial<ConnectionManagerConfig>) {
    this.config = { ...DEFAULT_CONNECTION_CONFIG, ...config };

    if (this.config.endpoints.length === 0) {
      throw new Error("At least one RPC endpoint is required");
    }

    this.activeEndpoint = this.config.endpoints[0];

    // Initialize connections and health state for each endpoint
    for (const url of this.config.endpoints) {
      const connection = new Connection(url, {
        commitment: this.config.commitment,
        confirmTransactionInitialTimeout: this.config.requestTimeoutMs,
      });
      this.connections.set(url, connection);

      this.healthStatus.set(url, {
        url,
        healthy: true,
        latencyMs: 0,
        lastChecked: 0,
        consecutiveFailures: 0,
      });

      this.rateLimiters.set(url, {
        tokens: this.config.maxRequestsPerSecond,
        maxTokens: this.config.maxRequestsPerSecond,
        refillRate: this.config.maxRequestsPerSecond,
        lastRefill: Date.now(),
      });

      this.requestCounts.set(url, []);
    }

    // Start automatic health checks if enabled
    if (this.config.autoHealthCheck && this.config.endpoints.length > 1) {
      this.startHealthChecks();
    }
  }

  /**
   * Get the currently active connection.
   */
  getConnection(): Connection {
    const conn = this.connections.get(this.activeEndpoint);
    if (!conn) {
      throw new Error(`No connection for endpoint: ${this.activeEndpoint}`);
    }
    return conn;
  }

  /**
   * Get the URL of the currently active endpoint.
   */
  getActiveEndpoint(): string {
    return this.activeEndpoint;
  }

  /**
   * Get health status for all endpoints.
   */
  getHealthStatus(): EndpointHealth[] {
    return Array.from(this.healthStatus.values());
  }

  /**
   * Get the connection for a specific endpoint.
   */
  getConnectionForEndpoint(url: string): Connection | undefined {
    return this.connections.get(url);
  }

  /**
   * Manually switch to a specific endpoint.
   */
  switchEndpoint(url: string): void {
    if (!this.connections.has(url)) {
      throw new Error(`Unknown endpoint: ${url}`);
    }
    this.activeEndpoint = url;
  }

  /**
   * Check if the rate limiter allows a request to the active endpoint.
   */
  canMakeRequest(endpoint?: string): boolean {
    const url = endpoint ?? this.activeEndpoint;
    const limiter = this.rateLimiters.get(url);
    if (!limiter) return false;

    this.refillTokens(limiter);
    return limiter.tokens >= 1;
  }

  /**
   * Consume a rate limit token for the active endpoint.
   * Returns true if the request is allowed.
   */
  consumeToken(endpoint?: string): boolean {
    const url = endpoint ?? this.activeEndpoint;
    const limiter = this.rateLimiters.get(url);
    if (!limiter) return false;

    this.refillTokens(limiter);

    if (limiter.tokens >= 1) {
      limiter.tokens -= 1;
      return true;
    }
    return false;
  }

  /**
   * Refill rate limiter tokens based on elapsed time.
   */
  private refillTokens(limiter: RateLimiterState): void {
    const now = Date.now();
    const elapsed = (now - limiter.lastRefill) / 1000;
    const tokensToAdd = elapsed * limiter.refillRate;
    limiter.tokens = Math.min(limiter.maxTokens, limiter.tokens + tokensToAdd);
    limiter.lastRefill = now;
  }

  /**
   * Wait until a rate limit token is available.
   */
  async waitForToken(endpoint?: string): Promise<void> {
    const url = endpoint ?? this.activeEndpoint;
    while (!this.consumeToken(url)) {
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
  }

  /**
   * Run a health check against a specific endpoint.
   */
  async checkEndpointHealth(url: string): Promise<EndpointHealth> {
    const connection = this.connections.get(url);
    if (!connection) {
      return {
        url,
        healthy: false,
        latencyMs: -1,
        lastChecked: Date.now(),
        consecutiveFailures: 999,
      };
    }

    const startTime = Date.now();

    try {
      const slot = await connection.getSlot();
      const latencyMs = Date.now() - startTime;

      const health: EndpointHealth = {
        url,
        healthy: true,
        latencyMs,
        lastChecked: Date.now(),
        consecutiveFailures: 0,
        slot,
      };

      this.healthStatus.set(url, health);
      return health;
    } catch {
      const existing = this.healthStatus.get(url);
      const consecutiveFailures = (existing?.consecutiveFailures ?? 0) + 1;

      const health: EndpointHealth = {
        url,
        healthy: consecutiveFailures < this.config.maxConsecutiveFailures,
        latencyMs: Date.now() - startTime,
        lastChecked: Date.now(),
        consecutiveFailures,
        slot: existing?.slot,
      };

      this.healthStatus.set(url, health);

      // Failover if the active endpoint is now unhealthy
      if (
        url === this.activeEndpoint &&
        !health.healthy &&
        this.config.autoFailover
      ) {
        this.failover();
      }

      return health;
    }
  }

  /**
   * Run health checks against all endpoints.
   */
  async checkAllEndpoints(): Promise<EndpointHealth[]> {
    const checks = this.config.endpoints.map((url) =>
      this.checkEndpointHealth(url)
    );
    return Promise.all(checks);
  }

  /**
   * Switch to the next healthy endpoint.
   */
  private failover(): void {
    const healthyEndpoints = this.config.endpoints.filter((url) => {
      const health = this.healthStatus.get(url);
      return health?.healthy && url !== this.activeEndpoint;
    });

    if (healthyEndpoints.length === 0) {
      // No healthy alternatives; try the first endpoint as fallback
      const fallback = this.config.endpoints.find(
        (url) => url !== this.activeEndpoint
      );
      if (fallback) {
        this.activeEndpoint = fallback;
      }
      return;
    }

    // Pick the healthiest (lowest latency) endpoint
    healthyEndpoints.sort((a, b) => {
      const healthA = this.healthStatus.get(a);
      const healthB = this.healthStatus.get(b);
      return (healthA?.latencyMs ?? Infinity) - (healthB?.latencyMs ?? Infinity);
    });

    this.activeEndpoint = healthyEndpoints[0];
  }

  /**
   * Start periodic health checks.
   */
  startHealthChecks(): void {
    if (this.healthCheckTimer) return;

    this.healthCheckTimer = setInterval(() => {
      this.checkAllEndpoints().catch(() => {
        // Health check errors are non-fatal
      });
    }, this.config.healthCheckIntervalMs);

    // Run an immediate check
    this.checkAllEndpoints().catch(() => {});
  }

  /**
   * Stop periodic health checks.
   */
  stopHealthChecks(): void {
    if (this.healthCheckTimer) {
      clearInterval(this.healthCheckTimer);
      this.healthCheckTimer = null;
    }
  }

  /**
   * Add a new endpoint at runtime.
   */
  addEndpoint(url: string): void {
    if (this.connections.has(url)) return;

    const connection = new Connection(url, {
      commitment: this.config.commitment,
      confirmTransactionInitialTimeout: this.config.requestTimeoutMs,
    });

    this.connections.set(url, connection);
    this.config.endpoints.push(url);

    this.healthStatus.set(url, {
      url,
      healthy: true,
      latencyMs: 0,
      lastChecked: 0,
      consecutiveFailures: 0,
    });

    this.rateLimiters.set(url, {
      tokens: this.config.maxRequestsPerSecond,
      maxTokens: this.config.maxRequestsPerSecond,
      refillRate: this.config.maxRequestsPerSecond,
      lastRefill: Date.now(),
    });
  }

  /**
   * Remove an endpoint at runtime.
   */
  removeEndpoint(url: string): void {
    if (this.config.endpoints.length <= 1) {
      throw new Error("Cannot remove the last endpoint");
    }

    this.connections.delete(url);
    this.healthStatus.delete(url);
    this.rateLimiters.delete(url);
    this.config.endpoints = this.config.endpoints.filter((e) => e !== url);

    if (this.activeEndpoint === url) {
      this.activeEndpoint = this.config.endpoints[0];
    }
  }

  /**
   * Get aggregate statistics across all endpoints.
   */
  getStats(): {
    totalEndpoints: number;
    healthyEndpoints: number;
    activeEndpoint: string;
    averageLatencyMs: number;
  } {
    const healths = Array.from(this.healthStatus.values());
    const healthy = healths.filter((h) => h.healthy);
    const avgLatency =
      healthy.length > 0
        ? healthy.reduce((sum, h) => sum + h.latencyMs, 0) / healthy.length
        : -1;

    return {
      totalEndpoints: this.config.endpoints.length,
      healthyEndpoints: healthy.length,
      activeEndpoint: this.activeEndpoint,
      averageLatencyMs: Math.round(avgLatency),
    };
  }

  /**
   * Destroy the manager and release resources.
   */
  destroy(): void {
    this.stopHealthChecks();
    this.connections.clear();
    this.healthStatus.clear();
    this.rateLimiters.clear();
  }
}
