name: Explorer_production_build&test
on:
  push:
   branches: [master]
   paths:
      - 'explorer/**'
jobs:
    Explorer_production_build_test:
      runs-on: ubuntu-latest
      defaults:
        run:
          working-directory: explorer
      steps:
       - uses: actions/checkout@v3
         with:
          ref: ${{ github.event.pull_request.head.sha }}
       - uses: actions/setup-node@v3
         with:
           node-version: '14'
           cache: 'npm'
           cache-dependency-path: explorer/package-lock.json
       - run: npm i -g npm@7
       - run: npm ci
       - run: npm run format
       - run: npm run build
       - run: npm run test
       
    Explorer_production_deploy:
      needs: Explorer_production_build_test
      runs-on: ubuntu-latest
      defaults:
        run:
            working-directory: explorer
    
      steps:
       - uses: actions/checkout@v3
         with:
           ref: ${{ github.event.pull_request.head.sha }}
       - uses: amondnet/vercel-action@v20
         with:
           vercel-token: ${{ secrets.VERCEL_TOKEN }} # Required
           github-token: ${{ secrets.PAT }} #Optional 
           vercel-args: '--prod' #for production
           vercel-org-id: ${{ secrets.ORG_ID}}  #Required
           vercel-project-id: ${{ secrets.PROJECT_ID}} #Required 
           scope: ${{ secrets.TEAM_ID }}
