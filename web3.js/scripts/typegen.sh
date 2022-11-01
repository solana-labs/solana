set -e

# Generate typescript declarations
npx tsc -p tsconfig.d.json -d

# Flatten typescript declarations
npx rollup -c rollup.config.types.js

# Replace export with closing brace for module declaration
sed -i.bak '$s/export {.*};/}/' lib/index.d.ts

# Replace declare's with export's
sed -i.bak 's/declare/export/g' lib/index.d.ts

# Prepend declare module line
sed -i.bak '2s;^;declare module "@solana/web3.js" {\n;' lib/index.d.ts

# Remove backup file from `sed` above
rm lib/index.d.ts.bak

# Run prettier
npx prettier --write lib/index.d.ts

# Check result
npx tsc lib/index.d.ts
