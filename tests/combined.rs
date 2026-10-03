use dekopon_provider_sdk_testkit::{Harness, Native, conformance};
use dekopon_python_provider::PythonProvider;
use serde_json::{Value, json};
use std::path::PathBuf;

fn component() -> PathBuf {
    PathBuf::from(std::env::var_os("DEKOPON_PROVIDER_COMPONENT").expect("built component path"))
}

fn eval(script: &str) -> Value {
    let output = Harness::<PythonProvider>::get(component())
        .call("python.eval", json!({"script":script}))
        .expect("broker invocation");
    assert_eq!(output.status, 0, "{}", output.stderr);
    serde_json::from_slice(&output.stdout).expect("JSON response")
}

#[test]
fn broker_runs_combined_python_sql_numeric_and_denials() {
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(|| {
    let joined = eval(
        r#"
import dekopon_tables as t
result = t.query('SELECT p.grp, SUM(s.score) AS total, ROW_NUMBER() OVER (ORDER BY SUM(s.score) DESC) AS rank FROM people p JOIN scores s ON p.id = s.id GROUP BY p.grp ORDER BY total DESC', {'people':[{'id':1,'grp':'a'},{'id':2,'grp':'a'},{'id':3,'grp':'b'}], 'scores':[{'id':1,'score':10},{'id':2,'score':20},{'id':3,'score':30}]})
"#,
    );
    assert_eq!(joined["ok"], true, "{joined}");
    assert_eq!(joined["result"]["columns"], json!(["grp", "total", "rank"]));
    assert_eq!(joined["result"]["rows"].as_array().unwrap().len(), 2);
    let empty = eval(
        "import dekopon_tables as t\nresult=t.query('SELECT id FROM t WHERE id < 0', {'t':[{'id':1}]})",
    );
    assert_eq!(empty["result"], json!({"columns":["id"],"rows":[]}));
    let clock = eval(
        "import dekopon_tables as t\nresult=t.query('SELECT CURRENT_TIMESTAMP AS ts, now() AS n, uuid() AS id FROM t', {'t':[{'id':1},{'id':2},{'id':3}]})",
    );
    assert_eq!(clock["ok"], true, "{clock}");
    let rows = clock["result"]["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 3);
    assert!(
        rows.iter().all(|row| row[0] == rows[0][0]
            && row[1] == rows[0][0]
            && row[2]
                .as_str()
                .is_some_and(|id| id.len() == 36 && id.as_bytes()[14] == b'4')),
        "{clock}"
    );
    let numeric = eval(
        r#"
import dekopon_numeric as n
result = {'sum':n.vector_add([1.,2.],[3.,4.]), 'dot':n.dot((1.,2.),(3.,4.)), 'product':n.matrix_multiply([[1.,2.],[3.,4.]],[[1.,0.],[0.,1.]]), 'means':n.column_means([[1.,2.],[3.,4.]]), 'cdf':n.normal_cdf(0.,0.,1.), 'prediction':n.linear_regression([[1.,1.],[2.,1.],[3.,1.],[4.,1.]],[3.,5.,7.,9.]), 'labels':n.kmeans([[0.,0.],[0.,1.],[10.,10.],[10.,11.]],2,42)}
"#,
    );
    assert_eq!(numeric["ok"], true, "{numeric}");
    assert_eq!(numeric["result"]["dot"], 11.0);
    assert_eq!(numeric["result"]["cdf"], 0.5);
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
        let denial = eval(script);
        assert_eq!(denial["ok"], false, "{script}: {denial}");
    }
    for script in [
        r#"import dekopon_tables as t
result=t.query("COPY t TO '/tmp/x'", {'t':[{'id':1}]})"#,
        r#"import dekopon_tables as t
result=t.query("SELECT * FROM read_csv('/tmp/x')", {'t':[{'id':1}]})"#,
    ] {
        let denial = eval(script);
        assert_eq!(denial["ok"], false, "{script}: {denial}");
        assert_eq!(denial["error"]["kind"], "runtime", "{script}: {denial}");
        assert_eq!(denial["error"]["type"], "ValueError", "{script}: {denial}");
    }
    assert_eq!(eval("result = 'x' * 131073")["error"]["kind"], "result");
    let command = Native::<PythonProvider>::new()
        .call("python.eval", &json!({"script":"result=1"}).to_string());
    assert_eq!(command.status, 0, "{}", command.stderr);
        })
        .expect("combined test thread")
        .join()
        .expect("combined test invocation");
}

#[test]
fn component_has_exact_toolkit_authority() {
    conformance::<PythonProvider>(component())
        .expect("typed declaration matches decoded imports and manifest");
}
