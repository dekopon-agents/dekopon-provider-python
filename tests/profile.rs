use dekopon_provider_sdk_testkit::{BrokerHostLimits, Harness};
use dekopon_python_provider::PythonProvider;
use serde_json::{Value, json};
use std::{path::PathBuf, time::Duration};

fn limits(memory_mib: usize) -> BrokerHostLimits {
    BrokerHostLimits {
        max_memory_bytes: memory_mib * 1024 * 1024,
        fuel: 24_000_000_000,
        max_timeout: Duration::from_secs(300),
        max_input_bytes: 12_582_912,
        max_output_bytes: 12_582_912,
        ..BrokerHostLimits::default()
    }
}

#[test]
#[ignore = "manual broker memory evidence for the Pi profile"]
fn both_toolkit_workloads_pass_at_64_or_128_mib_per_memory() {
    let component = PathBuf::from(
        std::env::var_os("DEKOPON_PROVIDER_COMPONENT").expect("built component path"),
    );
    for (name, script) in [
        (
            "join-window",
            "import dekopon_tables as t\nresult=t.query('SELECT p.grp, SUM(s.score) AS total, ROW_NUMBER() OVER (ORDER BY SUM(s.score) DESC) AS rank FROM people p JOIN scores s ON p.id = s.id GROUP BY p.grp ORDER BY total DESC', {'people':[{'id':1,'grp':'a'},{'id':2,'grp':'a'},{'id':3,'grp':'b'}], 'scores':[{'id':1,'score':10},{'id':2,'score':20},{'id':3,'score':30}]})",
        ),
        (
            "numeric",
            "import dekopon_numeric as n\nresult={'sum':n.vector_add([1.,2.],[3.,4.]),'dot':n.dot((1.,2.),(3.,4.)),'product':n.matrix_multiply([[1.,2.],[3.,4.]],[[1.,0.],[0.,1.]]),'means':n.column_means([[1.,2.],[3.,4.]]),'cdf':n.normal_cdf(0.,0.,1.),'prediction':n.linear_regression([[1.,1.],[2.,1.],[3.,1.],[4.,1.]],[3.,5.,7.,9.]),'labels':n.kmeans([[0.,0.],[0.,1.],[10.,10.],[10.,11.]],2,42)}",
        ),
    ] {
        let mut succeeded = false;
        for memory_mib in [64, 128] {
            let outcome = Harness::<PythonProvider>::get(&component)
                .host_limits(limits(memory_mib))
                .call("python.eval", json!({"script": script}));
            let passed = match outcome {
                Ok(output) if output.status == 0 => {
                    match serde_json::from_slice::<Value>(&output.stdout) {
                        Ok(data) => {
                            let valid = data["ok"] == true
                                && match name {
                                    "join-window" => data["result"]["rows"]
                                        .as_array()
                                        .is_some_and(|rows| rows.len() == 2),
                                    "numeric" => data["result"]["dot"] == 11.0,
                                    _ => unreachable!(),
                                };
                            if !valid {
                                eprintln!("{name} {memory_mib} MiB: unexpected result: {data}");
                            }
                            valid
                        }
                        Err(error) => {
                            eprintln!("{name} {memory_mib} MiB: invalid JSON: {error}");
                            false
                        }
                    }
                }
                Ok(output) => {
                    eprintln!(
                        "{name} {memory_mib} MiB: guest status {}: {}",
                        output.status, output.stderr
                    );
                    false
                }
                Err(error) => {
                    eprintln!("{name} {memory_mib} MiB: host refusal: {error}");
                    false
                }
            };
            println!(
                "{name} {memory_mib} MiB: {}",
                if passed { "PASS" } else { "FAIL" }
            );
            if passed {
                succeeded = true;
                break;
            }
        }
        assert!(succeeded, "{name} failed at both memory limits");
    }
}
