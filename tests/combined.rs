//! Combined candidate exercised in the actual broker-host/testkit, never via a native-only VM.
use dekopon_provider_sdk_testkit::{BrokerHostLimits, FakeBroker};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

mod support;

#[tokio::test(flavor = "multi_thread")]
async fn broker_runs_combined_python_sql_numeric_and_denials()
-> Result<(), Box<dyn std::error::Error>> {
    let component = PathBuf::from(
        std::env::var_os("DEKOPON_PROVIDER_COMPONENT")
            .expect("DEKOPON_PROVIDER_COMPONENT must point at the built combined component"),
    );
    let broker = support::build_broker(
        FakeBroker::builder()
            .component(component)
            .provider("python")
            .host_limits(BrokerHostLimits {
                max_memory_bytes: 256 * 1024 * 1024,
                fuel: 8_000_000_000,
                max_timeout: Duration::from_secs(30),
                ..BrokerHostLimits::default()
            })
            .timeout_ms(30_000)
            .max_output_bytes(786_432),
    )
    .await?;
    async fn eval(broker: &FakeBroker, script: &str) -> Result<Value, Box<dyn std::error::Error>> {
        Ok(broker
            .invoke("python.eval", json!({"script":script}))
            .await?)
    }
    let sql_started = Instant::now();
    let joined = eval(&broker, r#"
import dekopon_tables as t
result = t.query('SELECT p.grp, SUM(s.score) AS total, ROW_NUMBER() OVER (ORDER BY SUM(s.score) DESC) AS rank FROM people p JOIN scores s ON p.id = s.id GROUP BY p.grp ORDER BY total DESC', {'people':[{'id':1,'grp':'a'},{'id':2,'grp':'a'},{'id':3,'grp':'b'}], 'scores':[{'id':1,'score':10},{'id':2,'score':20},{'id':3,'score':30}]})
"#).await?;
    assert_eq!(joined["ok"], true, "{joined}");
    assert_eq!(joined["result"]["columns"], json!(["grp", "total", "rank"]));
    assert_eq!(joined["result"]["rows"].as_array().unwrap().len(), 2);
    let empty = eval(
        &broker,
        r#"
import dekopon_tables as t
result = t.query('SELECT id FROM t WHERE id < 0', {'t':[{'id':1}]})
"#,
    )
    .await?;
    assert_eq!(empty["ok"], true, "{empty}");
    assert_eq!(empty["result"], json!({"columns":["id"],"rows":[]}));
    eprintln!(
        "joined/grouped/window SQL invoke: {:?}",
        sql_started.elapsed()
    );
    let time_result = eval(&broker, r#"
import dekopon_tables as t
result=t.query('SELECT CURRENT_TIMESTAMP AS ts, now() AS n, uuid() AS id FROM t', {'t':[{'id':1},{'id':2},{'id':3}]})
"#).await?;
    assert_eq!(time_result["ok"], true, "{time_result}");
    let rows = time_result["result"]["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 3);
    assert!(
        rows.iter()
            .all(|row| row[0] == rows[0][0] && row[1] == rows[0][0]),
        "{time_result}"
    );
    assert!(
        rows.iter().all(|row| row[2]
            .as_str()
            .is_some_and(|id| id.len() == 36 && id.as_bytes()[14] == b'4')),
        "{time_result}"
    );
    let numeric_started = Instant::now();
    let numeric = eval(&broker, r#"
import dekopon_numeric as n
result = {'sum':n.vector_add([1.,2.],[3.,4.]), 'dot':n.dot((1.,2.),(3.,4.)), 'product':n.matrix_multiply([[1.,2.],[3.,4.]],[[1.,0.],[0.,1.]]), 'means':n.column_means([[1.,2.],[3.,4.]]), 'cdf':n.normal_cdf(0.,0.,1.), 'prediction':n.linear_regression([[1.,1.],[2.,1.],[3.,1.],[4.,1.]],[3.,5.,7.,9.]), 'labels':n.kmeans([[0.,0.],[0.,1.],[10.,10.],[10.,11.]],2,42)}
"#).await?;
    assert_eq!(numeric["ok"], true, "{numeric}");
    assert_eq!(numeric["result"]["dot"], 11.0);
    assert_eq!(numeric["result"]["cdf"], 0.5);
    eprintln!(
        "ndarray/statrs/SmartCore numeric invoke: {:?}",
        numeric_started.elapsed()
    );
    assert_eq!(
        numeric["result"]["labels"][0],
        numeric["result"]["labels"][1]
    );
    for script in [
        "import os",
        "import sys",
        "import datetime",
        "import socket",
        "import json.decoder",
        "import dekopon_tables as t\nresult=t.query('COPY t TO \'/tmp/x\'', {'t':[{'id':1}]})",
        "import dekopon_tables as t\nresult=t.query('SELECT * FROM read_csv(\'/tmp/x\')', {'t':[{'id':1}]})",
        "import dekopon_tables as t\nresult=t.query('SELECT * FROM t; DROP TABLE t', {'t':[{'id':1}]})",
        "import dekopon_tables as t\nresult=t.query('SELECT * FROM t', {'t':[{'id':1},{'id':'bad'}]})",
        "import dekopon_tables as t\nresult=t.query('SELECT * FROM t', {'t':[{'id':1}]*257})",
        "import dekopon_numeric as n\nresult=n.dot([1.],[1.,2.])",
        "import dekopon_numeric as n\nresult=n.matrix_multiply([[1.,2.]],[[3.]])",
        "import dekopon_numeric as n\nresult=n.normal_cdf(0.,0.,0.)",
        "import dekopon_numeric as n\nresult=n.column_means([[1.],[1.,2.]])",
        "import dekopon_numeric as n\nresult=n.kmeans([[1.],[1.]],2,42)",
        "import dekopon_numeric as n\nresult=n.vector_add([1.]*4097,[1.]*4097)",
        "import dekopon_numeric as n\nresult=n.vector_add([1e309],[1.])",
        "import dekopon_numeric as n\nresult=n.normal_cdf(True,0.,1.)",
    ] {
        let denial = eval(&broker, script).await?;
        assert_eq!(denial["ok"], false, "{script}: {denial}");
    }
    let capped = eval(&broker, "result = 'x' * 131073").await?;
    assert_eq!(capped["error"]["kind"], "result");
    let command = broker
        .run_command("python", &["python".to_owned(), "--help".to_owned()], None)
        .await?;
    assert!(
        matches!(
            command,
            dekopon_provider_sdk_testkit::CommandRunOutcome::Rendered { .. }
        ),
        "{command:?}"
    );
    Ok(())
}
