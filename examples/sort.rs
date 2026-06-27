use mozart::mozart;

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

fn main() {}
