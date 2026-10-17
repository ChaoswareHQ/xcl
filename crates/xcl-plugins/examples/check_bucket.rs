use xcl_plugins::new_vm;

fn main() {
    let vm = new_vm();
    
    // Check if bucket.create is registered globally
    println!("Checking bucket.create registration:");
    
    if let Some(builtin) = vm.resolve_builtin("bucket.create") {
        println!("  ✓ bucket.create found in global table");
    } else {
        println!("  ✗ bucket.create NOT found in global table");
    }
    
    // Check bucket domain
    println!("\nChecking bucket domain:");
    if let Some(&didx) = vm.domain_index.get("bucket") {
        let slot = &vm.domain_table[didx as usize];
        println!("  ✓ bucket domain found, methods:");
        for (method, &midx) in &slot.index {
            println!("    - {} (idx {})", method, midx);
        }
    } else {
        println!("  ✗ bucket domain NOT found");
    }
    
    // Check cloudflare domain
    println!("\nChecking cloudflare domain:");
    if let Some(&didx) = vm.domain_index.get("cloudflare") {
        let slot = &vm.domain_table[didx as usize];
        println!("  ✓ cloudflare domain found, methods:");
        for (method, &midx) in &slot.index {
            println!("    - {} (idx {})", method, midx);
        }
    } else {
        println!("  ✗ cloudflare domain NOT found");
    }
    
    // List all registered domains
    println!("\nAll registered domains:");
    for (name, _) in &vm.domain_index {
        println!("  - {}", name);
    }
    
    // Try to evaluate bucket.create without context
    println!("\nEvaluating bucket.create without context:");
    let mut vm = new_vm();
    let result = xcl_vm::evaluate(r#"bucket.create "test""#, &mut vm);
    println!("  Is nil: {}", result.is_nil());
    println!("  Is map: {}", result.as_map().is_some());
    if let Some(map) = result.as_map() {
        println!("  Map keys: {:?}", map.keys().collect::<Vec<_>>());
        if let Some(err) = map.get("error") {
            if let Some(err_map) = err.as_map() {
                if let Some(msg) = err_map.get("message") {
                    println!("  Error message: {}", msg.as_str().unwrap_or("(nil)"));
                }
            }
        }
    }
    
    // Try with hardcoded account
    println!("\nEvaluating bucket.create with hardcoded account:");
    let mut vm = new_vm();
    let result = xcl_vm::evaluate(r#"
        cloudflare "43bfb2aec65615fd15522c93f8297034" {
            bucket.create "xcl-test-bucket"
        }
    "#, &mut vm);
    print_result(&result);

    // Try with empty account (should load from .env)
    println!("\nEvaluating bucket.create with empty account (from .env):");
    let mut vm = new_vm();
    let result = xcl_vm::evaluate(r#"
        cloudflare "" {
            $config = cloudflare.config
            print "Account:" $config.account
            bucket.create "xcl-test-bucket"
        }
    "#, &mut vm);
    print_result(&result);
}

fn print_result(result: &xcl_vm::Value) {
    println!("  Is nil: {}", result.is_nil());
    println!("  Is map: {}", result.as_map().is_some());
    if let Some(map) = result.as_map() {
        println!("  Map keys: {:?}", map.keys().collect::<Vec<_>>());
        if let Some(err) = map.get("error") {
            if let Some(err_map) = err.as_map() {
                if let Some(msg) = err_map.get("message") {
                    println!("  Error message: {}", msg.as_str().unwrap_or("(nil)"));
                }
            }
        }
        if let Some(val) = map.get("value") {
            println!("  Value: {}", val.as_str().unwrap_or("(not a string)"));
        }
    }
}
