const LOKI_URL_FOR_NODE: &str =
  "https://grafana.teleport.parity.io/explore?orgId=1&left=%7B%22datasource%22:%22PCF9DACBDF30E12B3%22,%22queries%22:%5B%7B%22refId%22:%22A%22,%22datasource%22:%7B%22type%22:%22loki%22,%22uid%22:%22PCF9DACBDF30E12B3%22%7D,%22editorMode%22:%22code%22,%22expr%22:%22%7Bnamespace%3D%5C%22{{namespace}}%5C%22,pod%3D%5C%22{{podName}}%5C%22%7D%22,%22queryType%22:%22range%22%7D%5D,%22range%22:%7B%22from%22:%22{{from}}%22,%22to%22:%22{{to}}%22%7D%7D";

pub fn get_loki_url(namespace: &str, pod_name: &str, from: u128, to: Option<u128>) -> String {
    let loki_url = LOKI_URL_FOR_NODE
        .replace("{{namespace}}", &namespace)
        .replace("{{podName}}", &pod_name)
        .replace("{{from}}", &from.to_string())
        .replace(
            "{{to}}",
            to.map(|n| n.to_string())
                .unwrap_or("now".to_string())
                .as_str(),
        );

    loki_url
}
