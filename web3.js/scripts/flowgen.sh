set -ex

# Generate flowdef
npx flowgen lib/types/index.d.ts -o module.flow.js

# Remove trailing export
sed -i '/declare export {/{:a;N;/}/!ba};//g' module.flow.js

# Change all declarations to exports
sed -i 's/declare/declare export/g' module.flow.js

# Prepend declare module line
sed -i '7s;^;declare module "@solana/web3.js" {\n;' module.flow.js

# Append closing brace for module declaration
echo '}' >> module.flow.js
