# ivza-sdk

TypeScript SDK for IVZA — Execution Layer for Solana.

## Install

```bash
npm install ivza-sdk
```

## Usage

```typescript
import { IvzaClient } from "ivza-sdk";

const client = new IvzaClient(connection, wallet);

// intent DSL -> transaction graph -> parallel execution plan
const plan = await client.processIntent(`
  swap 500 USDC to SOL
  stake 50% of output SOL
`);

const results = await client.executeLocally(plan);
```

## Features

- Transaction graph builder with fluent API
- Intent DSL parser (deterministic, no LLM)
- Dependency analysis and parallel lane scheduling
- Parallel executor with retry and backoff
- Jito bundle builder
- Connection manager with failover

## Documentation

- [Docs](https://ivza.dev/docs)
- [SDK Reference](https://ivza.dev/docs/sdk)
- [GitHub](https://github.com/ivzadotdev/ivza)

## License

MIT
