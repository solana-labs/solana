# build environment
FROM node:16-alpine as react-build
WORKDIR /app
COPY ./package.json ./package-lock.json ./
RUN npm install
COPY . ./
RUN npm run build

# server environment
FROM nginx:alpine

COPY --from=react-build /app/build /usr/share/nginx/html

ENV PORT 80
ENV HOST 0.0.0.0
EXPOSE 80

CMD ["nginx", "-g", "daemon off;"]
