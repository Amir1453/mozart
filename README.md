# Mozart 

Compose Rust function variations like a boss.

Mozart is an experimental Rust procedural macro for generating multiple versions of a function from a set of compile-time variants. It is useful when you want to write one function template and expand it into every combination of several implementation choices. For example:

```rust
mozart! {
    sort => { <[_]>::sort, <[_]>::sort_unstable }

    integer => { u8, usize }

    block => {
        one => { println!("Cats"); },
        two => { 
            println!("Cats and boots");
            println!("And hongs");
        }
    }

    fn test(cat: &mut Vec<i32>) {
        variant![sort](cat);

        if size_of::<variant!(integer)>() > 4 {
            println!("Happy");
        }

        variant! { block }
    }
}
```

Mozart expands this into one function for each combination of the variants above. In this example, Mozart generates 8 distinct functions, since there are 3 variant sections with each of them having 2 variants.

The generated functions are exposed via an accessor `__variants_{name}::accessor()`, and the functions are stored in a module named `__variants_{name}`.


## Why use Mozart ?

Mozart is a good fit when you want to:

- Benchmark several implementations of the same function
- Test the same logic over multiple concrete types
- Compare algorithmic choices
- Avoid manually writing repetitive function variants

## Current limitations

Mozart currently does not support:

- Generic function declarations
- Lifetime parameters on the generated function
- async fn
- Methods with self
- Using the same variant group as more than one placeholder kind
