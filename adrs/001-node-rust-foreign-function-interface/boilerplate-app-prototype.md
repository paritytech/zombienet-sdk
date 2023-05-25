## [Back](001-node-to-rust-foreign-function-interface.md)

## Boilerplate app to execute prototypes

1. Create the new node app :

```bash
$ mkdir -p ffi-prototype/app && cd ffi-prototype/app && npm init -y
```

2. Install required packages :

```bash
[ffi-prototype/app]$ npm i -D @tsconfig/recommended ts-node typescript
```

3. Add a new script :

```json
{
  "scripts": {
    "build+exec": "tsc && node ./index.js"
  }
}
```

4. Add tsconfig.json
```json
{
  "extends": "@tsconfig/recommended/tsconfig.json"
}
```